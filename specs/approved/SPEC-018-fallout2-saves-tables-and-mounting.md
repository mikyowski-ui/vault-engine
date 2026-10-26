# SPEC-018: Fallout 2 Mounting, Saves and Data Tables

**Status:** approved. Section 1 is an owner decision of 2026-10-25; the rest by the Spec-Author sub-agent (Team A), 2026-10-25, under AGENTS.md "Spec Authoring (Team A)". Revision 2 (retail corpus audit) by the coordinator, 2026-10-25.

**Provenance:** Team A (the Spec-Author sub-agent), 2026-10-25, from the
coordinator's brief of the same day and the owner's archive-role decision
(section 1). Source categories:
- the project's approved specifications SPEC-001 to SPEC-015, and, for the
  reconciliation pass of the same day, SPEC-017 and SPEC-019;
- `docs/fallout2-and-qol-roadmap.md`;
- Team A's general knowledge of the publicly documented Fallout 2 data files
  (the names of the shipped archives and text tables, their section and key
  names) and of publicly observable game behavior.

No decompilation, disassembly, re-implementation source or game asset was
consulted. Team A's general knowledge comes from model training data whose
own sources cannot be audited, so this specification cannot certify that
none of its content traces back to reverse-engineered material (see
`PROVENANCE.md`, "In-session spec authoring (Team A)"). Every point that
Team A settled without a source is listed in section 10, **Decisions**. A
later research-vault revision of any part supersedes that part.

**Revision 2 (retail corpus audit), 2026-10-25.** The coordinator wrote this
revision at the owner's request. It is based on loading the owner's retail
Fallout 2 install (the GOG release) through this specification's own
parsers.

- **What was opened.** Only section 1's three archives, plus the
  install's loose `data` folder, which holds `worldmap.dat` and one tile FRM
  and nothing this specification reads. sfall's archive, `f2_res.dat` and
  the `mods` folder were neither mounted nor opened.
- **Results on the retail install.**
  - Section 2 accepts the install, with no required member missing.
  - Every required table loads.
  - The line tables give 110 quests, 20 holodisks, 9 reputation rows, 17
    karma variables and 52 endgame slides.
  - 7 walk masks load.
- **The discrepancy.** Section 4 gave 1123 drop-or-default diagnostics. Most
  of them come from the encounter grammar of section 4.4, which the
  retail file writes differently.
- **What changes.** Each change below is marked "(Revision 2)", and
  Decisions R7 to R13 record the measurements. No game text is reproduced
  beyond single tokens and field shapes.

## 1. Archive roles and mounting `[Must-match: owner decision, 2026-10-25]`

Under the Fallout 2 profile (`game = "fallout2"`), the game data is mounted as
in Fallout 1's SPEC-008 §3.2 database model, with SPEC-015 archives:

- `MASTER.DAT` is the **master database**. Every request except critter art
  reads it.
- `CRITTER.DAT` is the **critter-art database**. Requests under
  `art/critters/` read it, with no fallback to the master database, exactly
  as SPEC-008 §3.2 selects between Fallout 1's two archives.
- An optional `patch000.dat`, found beside `MASTER.DAT` (the name compared
  case-insensitively), is mounted **above** `MASTER.DAT` in the master
  database. A file present in both is read from `patch000.dat`; a file absent
  there falls through to `MASTER.DAT`. Its absence is not an error.
- The loose data folder (`data_root`), when set, is consulted first by both
  databases, as in SPEC-008 §3.2.
- Each archive is read as SPEC-015 specifies; a SPEC-008 archive is refused
  under this profile.

## 2. Install validation and patch discovery

Section 1 says which archive serves which request. This section says how the
launcher finds the archives and when it accepts a folder as a Fallout 2
install.

### 2.1 Finding the archives

The **install folder** is the Fallout 2 folder configured in the launcher.
The launcher lists that folder's own entries (not its subfolders) and matches
three names, each compared with ASCII letters folded to one case:
`MASTER.DAT`, `CRITTER.DAT` and `patch000.dat`. `[Safe rule]` (D4)

- Only a regular file matches; links are followed first. A folder or other
  entry with a matching name is skipped, as if absent.
- At most one entry may match each name. Two entries that differ only in
  case (possible on a case-sensitive file system), for example
  `patch000.dat` and `PATCH000.DAT`, refuse the install. The message names
  both entries.
- `MASTER.DAT` and `CRITTER.DAT` are required. A missing one refuses the
  install, naming it.
- `patch000.dat` is optional. It is looked for **only** in the install
  folder, which is the folder that holds `MASTER.DAT`: never in `data_root`,
  never in a subfolder, never under another name. No other name, such as
  `patch001.dat`, is mounted.
- Each found archive is opened as SPEC-015 specifies. Any SPEC-015 refusal
  refuses the install, naming the archive and the rule. This includes
  `patch000.dat`: a patch that is present but unreadable is an error, not
  skipped, because skipping it would silently run unpatched data.
- When `MASTER.DAT` is refused by SPEC-015 but opens as a SPEC-008 archive,
  the message says that the folder holds Fallout 1 data.

The engine does not read the archive-path keys of a Fallout 2 `fallout2.cfg`
(`master_dat`, `critter_dat`, `master_patches`, `critter_patches`). The
launcher's install folder and `data_root` settings take their place. (D5)

### 2.2 Required members

These are the Fallout 2 equivalents of Fallout 1's three required members
(`color.pal`, `data/vault13.gam` and the new-game map). Each is looked up in
the **master database as mounted** in section 1: `data_root` first, then
`patch000.dat`, then `MASTER.DAT`. The path is the SPEC-015 §4 canonical form
(lower case, `/`-separated). `[Safe rule]` (D6)

| Member | Used by |
| --- | --- |
| `color.pal` | The game palette (SPEC-001) |
| `data/vault13.gam` | The campaign initializer (section 6.2) |
| `maps/artemple.map` | The new-game map (section 6.5) |
| `data/maps.txt` | The map table (section 4.2) |
| `data/city.txt` | The area table (section 4.3) |
| `data/worldmap.txt` | The world grid and encounters (section 4.4) |
| `data/party.txt` | The party-member table (section 4.5) |

- The launcher checks presence only. A missing member refuses the install,
  naming it.
- **Play** parses the four tables of section 4 and the campaign initializer
  before anything else of the new game happens (section 6.6, step 1). A
  table that section 4 refuses stops Play with a message naming the file and
  line. No partial game starts.
- `data/quests.txt` and `data/holodisk.txt` (section 4.6) are optional. When
  absent, the Pip-Boy lists are empty.
- `CRITTER.DAT` must open, but no particular member is required in it.

### 2.3 Writable folder and slots

- The working map set (`MAPS\*.SAV`, `MAPS\AUTOMAP.DB`) and the save root
  `SAVEGAME\` live in the profile's writable data folder, as SPEC-014 §3.35
  says for Fallout 1. Nothing is ever written into an archive (SPEC-008
  §3.2).
- Slots belong to one game. A Fallout 1 slot never loads under the Fallout 2
  profile, and a Fallout 2 slot never loads under Fallout 1. The header
  check of section 7.2 enforces this for each slot, even when both profiles
  share a folder. `[Must-match: owner instruction]` (D3)

## 3. Conventions

### 3.1 Compatibility tags

- `[Must-match]`: the engine implements exactly what is written. Where the
  content comes from Team A's knowledge of the shipped Fallout 2 data, it is
  marked with a decision number (Dn) and remains subject to correction by
  the research vault or the local corpus audit.
- `[Engine layout]`: a binary layout this specification defines because
  Team A does not know the original one. The engine writes and reads exactly
  this layout. Interoperability with files written by the original Fallout 2
  executable is **not** claimed.
- `[Safe rule]`: what the engine does with malformed or unexpected input.
- `[May-improve]` keeps its SPEC-008 meaning, where a SPEC-008 rule is
  inherited.

"As SPEC-0xx §y" means the rule there applies unchanged under the Fallout 2
profile.

### 3.2 Numbers

- Save files and the automap database keep SPEC-008 §2.1's conventions:
  every multi-byte field is **big-endian**, signed values are two's
  complement. The little-endian rule of SPEC-015 covers archives only.
  `[Must-match]` (D33)
- Map streams follow SPEC-016.
- `random(a, b)` is an inclusive draw from the shared generator of SPEC-006
  §3.4. When a equals b, no draw is made. The draw order in every procedure
  below is part of the rule. `[Must-match]`

### 3.3 The table text grammar

`data/maps.txt`, `data/city.txt`, `data/worldmap.txt` and `data/party.txt` use
one sectioned key/value grammar. `[Must-match]` for what it accepts, (D7):

1. **Bytes.** A file is read as bytes, at most 64 MiB (the SPEC-015 member
   limit). A NUL byte anywhere refuses the file. Bytes `0x80`-`0xFF` are
   allowed in values and kept as single-byte text. No Unicode decoding is
   done.
2. **Lines.** Lines end at LF (`0x0A`). The last line may lack one. Any line
   longer than 4096 bytes refuses the file.
3. **Comments.** From the first `;` to the end of the line is a comment and
   is removed. These four files have no other comment marker.
4. **Trimming.** Leading and trailing space (`0x20`), tab (`0x09`) and CR
   (`0x0D`) are removed. A line that is then empty is skipped.
5. **Section headers.** A line that begins with `[` and ends with `]` opens a
   section. Its name is the text between them, trimmed. An empty name is
   ignored, with a diagnostic, and the lines after it belong to no section.
6. **Pairs.** Any other line that contains `=` is a pair. The key is the text
   before the first `=`, trimmed. The value is the text after it, trimmed,
   and may be empty. A pair with an empty key is ignored, with a diagnostic.
7. **Other lines.** A line that is neither a section header nor a pair is
   ignored, with a diagnostic. So is a pair before the first section.
8. **Case.** Section names, keys and the keywords inside values compare with
   ASCII case folded. Values themselves keep their case. Names inside
   values, such as map lookup names, also compare with case folded.
9. **Repeats.** Two sections with the same name (case folded) are one
   section; their pairs accumulate in file order. A key repeated within one
   section keeps the **later** value, with a diagnostic.
10. **Unknown sections and keys** are ignored without effect.
11. **Numbered sections and keys.** A numbered section name is a prefix, one
    or more spaces, then a decimal number with optional leading zeros:
    `[Map 012]`, `[Area 3]`, `[Tile 0]`, `[Encounter Table 2]`,
    `[Party Member 1]`. An indexed key is a base, `_`, then a decimal index:
    `entrance_0`, `enc_07`, `type_00`, `map_00`, `random_start_point_1`.
    `enc_7` and `enc_07` are the same key, so rule 9 applies to them. Records
    are identified by these numbers, never by file order.

### 3.4 Value forms

| Form | Accepted text | Notes |
| --- | --- | --- |
| Integer | Optional `+` or `-`, then 1 to 10 decimal digits, within the signed 32-bit range; nothing else | Stricter than SPEC-006's `.GAM` conversion: `12tail` and `0x10` are invalid here |
| Percent | An Integer 0-100, optionally followed by `%` | |
| Boolean | `Yes`/`No`, `On`/`Off`, `True`/`False`, `1`/`0` | Case folded |
| List | Items separated by `,` at parenthesis depth 0, each trimmed; empty items dropped | Commas inside `(...)` do not separate |
| Stem | 1 to 8 bytes of `A`-`Z`, `a`-`z`, `0`-`9`, `_`, `-`; a final `.map` (case folded) is removed first | File stems; 8 bytes keep `<stem>.SAV` within the 12-byte limit of SPEC-008 §2.5 |
| Name | 1 to 64 bytes, without `,`, `(`, `)`, `=` or `;` | Lookup names; may contain spaces |

`[Must-match]` (D7)

### 3.5 What happens to bad input

`[Safe rule]` (D7)

- **File refused.** The structural faults of section 3.3 rules 1 and 2, and
  the faults each table names as refusing. Play stops, naming the file and
  line.
- **Record dropped.** A record whose required key is missing or invalid
  does not exist. A later reference to it behaves as a reference to an
  absent record. A diagnostic names the record.
- **Value defaulted.** An optional key with an invalid value takes its
  default. A diagnostic names the key.

Diagnostics never stop play. A table is parsed once per Play or load, before
the game state changes.

## 4. Data tables

### 4.1 Overview

| File | Required | Takes the place of (Fallout 1) | Consumers |
| --- | --- | --- | --- |
| `data/maps.txt` | Yes | `map.msg` messages 0-65 as map numbers, and the SPEC-008 §3.10 town and music table | Sections 5.1-5.2, 7 |
| `data/city.txt` | Yes | The SPEC-008 §3.7 town and hotspot catalogs | Sections 5.2, 5.8, 7.4; SPEC-019 (world and town map screens) |
| `data/worldmap.txt` | Yes | The SPEC-008 §3.6 grids, §3.5 encounter tables and the §3.4 walk mask | Sections 5.3-5.7, 7.4 |
| `data/party.txt` | Yes | Nothing; Fallout 1 has no party configuration | Section 7.5; SPEC-019 (party control) |
| `data/quests.txt` | No | The SPEC-008 §3.8 quest table | Section 5.9 |
| `data/holodisk.txt` | No | The SPEC-008 §3.8 holodisk table | Section 5.9 |
| `data/genrep.txt`, `data/karmavar.txt` | No | The SPEC-014 §3.42 Karma folder rows | SPEC-019 §3.9 |
| `data/endgame.txt` | No | The SPEC-014 §3.30 slide table | SPEC-019 §3.20 |
| `data/vault-engine/elevators.txt` | No | The SPEC-013 §2.7 elevator table | Section 4.9 |

### 4.2 `data/maps.txt`: the map table

**Records.** Each `[Map NNN]` section, NNN from 0 to 999, is the record of
**map number** NNN. A number outside 0-999 drops the record. Numbers need not
be contiguous; an unused number is an **absent map**. Let `M_T` be the
highest number that has a record, plus 1. `[Must-match]` (D8)

| Key | Form | Default | Meaning |
| --- | --- | --- | --- |
| `map_name` | Stem | Required | The map file is `maps/<stem>.map`, the canonical lower-case path |
| `lookup_name` | Name | None | The name by which `city.txt` and `worldmap.txt` refer to this map |
| `music` | Stem, or empty | None | City music track, `sound/music/<music>.acm` (section 5.2) |
| `ambient_sfx` | List of `sound:weight` items, sound a Stem and weight an Integer 0-100 | Empty | Ambient sound choices; an invalid item is dropped. Parsed, with no effect in this revision |
| `saved` | Boolean | Yes | No: the map is in the excluded set (section 5.2) |
| `dead_bodies_age` | Boolean | Yes | No: the corpse-replacement revisit rule is skipped (section 5.2) |
| `can_rest_here` | List of exactly 3 Booleans, for elevations 0, 1, 2 | `Yes,Yes,Yes` | Rest permission per elevation (section 5.2); any other item count defaults |
| `pipboy_active` | Boolean | Yes | Whether the Pip-Boy may be opened on this map (SPEC-019 §3.10) |
| `random_start_point_N` | `elev:E, tile_num:T`, two fields in either order, E an Integer 0-2, T an Integer 0-39999 | None | Entry point N, N 0-99, used when the map is entered as an encounter (section 5.7). An invalid point is dropped |

`[Must-match]` (D8)

- **Duplicate names.** Two records whose stems are equal (case folded) both
  exist. Number-to-file lookups use each record's own stem. Name-to-number
  lookups (section 5.1) take the lowest number. Duplicate lookup names
  resolve the same way. Each duplicate gets a diagnostic. `[Safe rule]`
- **Refusal.** A file with no valid record is refused. `[Safe rule]`
- **Start points.** A map's start points are its valid
  `random_start_point_N` entries in ascending N, renumbered 0, 1, 2 and so
  on without gaps.

### 4.3 `data/city.txt`: areas and entrances

**Terminology.** An **area** here, in SPEC-017, and in `city.txt`'s own
section names, is the same thing as a **city** in SPEC-019: one `[Area NN]`
record. The area number is the city id. (R5)

**Records.** Each `[Area NN]` section, NN from 0 to 999, is the record of
**area number** NN. Let `A` be the highest valid area number plus 1. Numbers
below `A` without a valid record are **absent areas**: they never become
known and are never drawn. `[Must-match]` (D9)

| Key | Form | Default | Meaning |
| --- | --- | --- | --- |
| `world_pos` | `x,y`, two Integers with `0 <= x < world width` and `0 <= y < world height` (section 5.3) | Required | The area's world-map position, in world pixels |
| `area_name` | Any text, possibly empty | Empty | The area's **display name**: the name SPEC-019 draws on the world map, in the town list and on the save screen (section 5.2) |
| `start_state` | Boolean | Off | On: the area is known at a new game |
| `lock_state` | Boolean | Off | On: the area is locked at a new game (section 5.8) |
| `size` | `Small`, `Medium` or `Large` | `Medium` | The area's circle radius on the world map (SPEC-019 §2.6, §3.12.2) |
| `townmap_art_idx` | Integer, -1 or more | -1 | Index in `art/intrface/intrface.lst` of the town map picture; -1 means none (SPEC-019 §3.13) |
| `townmap_label_art_idx` | Integer, -1 or more | -1 | Index of the town map label art; -1 means none (SPEC-019 §3.12.3) |
| `entrance_N` | Seven fields, below | None | Entrance slot N, N from 0 to 31 |

An entrance value is a List of exactly seven items, in this order:
`[Must-match]` (D9)

| Item | Form | Meaning |
| ---: | --- | --- |
| 1 | Boolean | Known at a new game |
| 2 | Integer 0-1023, or -1 | X of the entrance marker on the town map picture; -1 with item 3 also -1 is an entrance with no marker (Revision 2, R12) |
| 3 | Integer 0-1023, or -1 | Y of the marker |
| 4 | Name | The `lookup_name` of the destination map in `maps.txt` |
| 5 | Integer -1 to 2 | Entry elevation; -1 takes the map header's elevation |
| 6 | Integer -1 to 39999 | Entry hex; -1 takes the map header's whole default entrance (hex, elevation and orientation), ignoring items 5 and 7 |
| 7 | Integer -1 to 5 | Entry orientation; -1 takes the map header's orientation |

- An entrance with the wrong item count, an invalid item, or a lookup name
  that no map has, is dropped with a diagnostic. `[Safe rule]`
- An entrance whose X and Y are both -1 is valid, and is never drawn on the
  town map. Scripts and section 5 still use it. Only one of the two at -1 is
  invalid. `[Safe rule]` (Revision 2, R12)
- An area's **entrance count** `E` is its highest valid slot number plus 1.
  A slot below `E` without a valid entrance is an absent slot, which is
  never known and never shown.
- **Refusal.** A file with no valid area is refused. `[Safe rule]`

### 4.4 `data/worldmap.txt`: the world grid and encounters

#### 4.4.1 `[Data]`

`[Must-match]` (D10)

- `terrain_types`: a List of `Name:difficulty` items, difficulty an Integer
  1-10. The position in the list is the **terrain index**, from 0. An
  invalid item is dropped. When the key is absent, or no item is valid, the
  list is `Desert:1, Mountain:2, City:1, Ocean:1`.
- Every other key of `[Data]` whose value is a Percent defines a **frequency
  name**: the key, case folded, with that percentage. The six names
  `Forced` 100, `Frequent` 38, `Common` 22, `Uncommon` 12, `Rare` 4 and
  `None` 0 exist by default; a key of the same name replaces the default
  value. Other `[Data]` keys are ignored.

#### 4.4.2 Tiles and subtiles

The world is built of **tiles** of 350 by 300 world pixels. Each tile holds 7
by 6 **subtiles** of 50 by 50 pixels. A subtile is the Fallout 2 equivalent
of SPEC-008's world cell. `[Must-match]` (D10)

- `[Tile Data]` holds `num_horizontal_tiles` `H`, an Integer 1-16. A missing
  or invalid value refuses the file.
- Each `[Tile N]` section, N from 0 to 255, describes tile N. Let `T` be the
  highest valid N plus 1. No valid tile refuses the file. A tile number
  below `T` without a section is a **default tile**, all of whose subtiles
  take the defaults below.
- Tile N sits in tile column `N mod H` and tile row `floor(N / H)`.

Keys of `[Tile N]`:

| Key | Form | Default | Meaning |
| --- | --- | --- | --- |
| `art_idx` | Integer, -1 or more | -1 | Index in `art/intrface/intrface.lst` of the tile's world-map picture (SPEC-019 §3.12.1) |
| `encounter_difficulty` | Integer | 0 | Stored and exposed; no rule in this specification reads it |
| `walk_mask_name` | Stem, or empty | None | The tile's walk mask, `data/<stem>.msk` (below) |
| `X_Y`, X 0-6, Y 0-5 | List of 5 or 6 items, below | Defaults below | Subtile X, Y of the tile, X counted rightward and Y downward |

A subtile value has these items, in order:

| Item | Form | Default when invalid or missing | Meaning |
| ---: | --- | --- | --- |
| 1 | A terrain name from `terrain_types` | Terrain index 0 | Terrain |
| 2 | `No_Fill`, `Fill_N`, `Fill_S`, `Fill_E`, `Fill_W`, `Fill_NE`, `Fill_NW`, `Fill_SE`, `Fill_SW` | `No_Fill` | Reveal direction (section 5.5) |
| 3 | A frequency name | `None` | Encounter frequency in the morning |
| 4 | A frequency name | `None` | In the afternoon |
| 5 | A frequency name | `None` | At night |
| 6 | The `lookup_name` of an encounter table, or absent | No table | The subtile's encounter table |

A subtile key that is absent, or whose value has another item count, takes
all six defaults, with a diagnostic. `[Safe rule]`

**Walk mask.** `[Engine layout]` (D12) A mask file is exactly 13200 bytes: 300
rows of 44 bytes, top row first. In a row, byte b covers tile-local pixels
`8b` to `8b + 7`, the most significant bit first. Bits for x of 350 or more
are ignored. A set bit blocks travel (section 5.4). A missing mask file, or
one of another size, blocks nothing, with a diagnostic.

#### 4.4.3 `[Random Maps: <terrain>]`

The section name is `Random Maps:` followed by a terrain name (trimmed, case
folded). Its `map_NN` keys, NN 0-99, hold map lookup names. The terrain's
**random map list** is the valid names in ascending NN. An unknown terrain
name ignores the section; an unknown map name drops that entry.
`[Must-match]` (D10)

#### 4.4.4 `[Encounter Table N]`

N is 0-999 and is the table's number. `[Must-match]` (D10, D11)

| Key | Form | Default | Meaning |
| --- | --- | --- | --- |
| `lookup_name` | Name | Required | The name subtiles use. Duplicates: the lowest table number wins, with a diagnostic |
| `maps` | List of map lookup names | Empty | The table's encounter maps; unknown names are dropped |
| `enc_NN`, NN 0-99 | Entry, below | None | Entry NN |

An **entry** value is a List of fields. Each field is one of:

| Field | Form | Default | Meaning |
| --- | --- | --- | --- |
| `Chance:<p>` | Percent | 0 | Selection weight |
| `Counter:<n>` | Integer -1 to 1000 | -1 | Remaining occurrences; -1 is unlimited |
| `Special` | The bare word | Not special | A special encounter (section 5.6) |
| `Map:<name>` | A map lookup name | None | Forces this map |
| `Enc:<expression>` | Group expression, below | No groups | What appears |
| `If(<condition>)` | Condition field, section 4.4.6 | Always true | Eligibility |

Keywords before `:` and the word `Special` are case folded. An unknown field
is ignored with a diagnostic; the entry stays. An invalid known field drops
the whole entry. An entry may have no `Enc:`: it then loads its map with
nothing added. `[Safe rule]`

**Group expressions.** `[Must-match]` (D11)

```text
expression := group { relation target }
group      := [ "(" count [ "-" count ] ")" ] groupname
relation   := "AND" | "FIGHTING" | "AMBUSH"
target     := group | "Player"
count      := Integer 0-25
groupname  := Name without spaces
```

- Spaces may separate any two tokens. Keywords are case folded.
- (Revision 2, R8) A group without a count is `(1)`. The retail file writes
  every special encounter's group this way, and so drops 375 entries
  without this rule.
- `(n)` means exactly n; `(a-b)` means from a to b. a greater than b drops
  the entry.
- `Player` may follow `AMBUSH` or `FIGHTING`, and `AMBUSH` must be followed by
  `Player`. Any other pairing drops the entry. (Revision 2, R8: the retail
  file has 3 entries ending `FIGHTING Player`.)
- `groupname` names an `[Encounter: <groupname>]` section (section 4.4.5).
  An unknown name is kept and skipped when spawning, with a diagnostic.

Example: `Enc:(2-4) ARRO_Geckos FIGHTING (1) ARRO_Hunters` is two groups, 2 to
4 and exactly 1, joined by FIGHTING.

#### 4.4.5 `[Encounter: <name>]`

The section name is `Encounter:` followed by the group name (trimmed, case
folded). `[Must-match]` (D11, D21)

- `position`: a List whose first item is a formation, `surrounding`,
  `straight_line`, `double_line`, `wedge`, `cone` or `huddle`, followed by
  optional `spacing:<n>`, an Integer 1-10, default 1, and `distance:<n>`, an
  Integer 0-30, or `Player(Perception)`, default 8. `Player(Perception)` is
  the player's Perception (SPEC-011), clamped to 0-30, when the group
  spawns (Revision 2, R10). Invalid items take their defaults. Section 5.7
  places every formation by one rule; the formation name is stored for
  SPEC-019.
- `team_num`: an Integer, stored with the group and given no engine effect,
  as for the relations of section 5.7. It is accepted without a diagnostic.
  (Revision 2, R10)
- `type_NN`, NN 0-99: a **critter type**, a List of fields:

| Field | Form | Meaning |
| --- | --- | --- |
| `pid:<n>` | Integer, a PID of category 0 (item), 1 (critter) or 2 (scenery) (SPEC-003 §2.1) | Required; without it, or with another category, the type is dropped. An item or scenery type places that object (section 5.7); its `Dead`, `Script` and `Item` fields are ignored with a diagnostic (Revision 2, R9) |
| `Ratio:<p>` | Percent | Selection weight (section 5.7) |
| `Dead` | Bare word | The critter is placed dead |
| `Script:<n>` | Integer -1 or more | Zero-based `scripts.lst` index of the critter's script; -1 or absent uses the prototype's script |
| `Item:<spec>` | Item spec, below; the field may repeat | Inventory added to the critter |
| `Distance:<n>` | Integer 0-30 | This type's placement distance, replacing the group's `distance` (section 5.7). An invalid value is ignored with a diagnostic (Revision 2, R10) |
| `If(<condition>)` | Condition field | The type is eligible only while it holds |

An item spec is `[ "(" count [ "-" count ] ")" ] pid [ "(wielded)" | "{wielded}" ]`,
with count an Integer 0-1000 and pid an item PID (category 0). The count
defaults to 1. `(wielded)` or `{wielded}`, with the word case folded, makes
the item the critter's held weapon (Revision 2, R11: 5 retail items use the
brace form). An invalid item
spec is dropped with a diagnostic; the type stays. `[Safe rule]`

#### 4.4.6 Conditions

`[Must-match]` (D11)

A **condition field** is one or more conditions, each wrapped in `If(...)`,
joined by `And` or `Or` (Revision 2, R7):

```text
field     := "If" "(" condition ")" { ("And" | "Or") "If" "(" condition ")" }
condition := clause { ("And" | "Or") clause }
clause    := operand compare Integer
           | "Rand(" Percent ")"
operand   := "Global(" Integer ")" | "Player(Level)"
           | "Days_Played" | "Time_Of_Day"
compare   := "==" | "=" | "!=" | "<" | "<=" | ">" | ">="
```

- Spaces may separate tokens, including between `If` and `(`. Keywords are
  case folded, so `time_of_day` is `Time_Of_Day`. `=` means `==`.
- The field is evaluated like one condition whose clauses are all of its
  conditions' clauses, in order. Each `And` or `Or` between two `If(...)`
  joins the last clause of one to the first clause of the next, under the
  evaluation rule below. (Revision 2, R7: the retail file writes 1004 of its
  1033 entry conditions as two or three `If(...)` joined by `And`. Read as
  one condition, they never held.)
- `Global(n)` is campaign global n; out of range reads 0 (SPEC-006 §2.1).
  `Player(Level)` is the player's level. `Days_Played` is
  `trunc(game ticks / 864000)`. `Time_Of_Day` is the HHMM value of SPEC-005
  §3.5.
- `Rand(p)` draws `random(1, 100)` and is true when the draw is at most p.
- **Evaluation** runs clause by clause, left to right, with no precedence
  and **no short-circuit**: every clause is evaluated, so every `Rand` draws.
  The running value starts as the first clause; each `And` or `Or` combines
  it with the next clause.
- A condition that does not match the grammar is false, permanently, with
  one diagnostic when the file is parsed. Its entry or type is then never
  eligible, and its clauses make no draws. `[Safe rule]`

### 4.5 `data/party.txt`: party members

Each `[Party Member N]` section, N from 0 to 99, is party record N. Let `P_T`
be the highest valid N plus 1. This specification defines the data and its
saved state (section 7.5); **SPEC-019 owns the behavior** (who may join,
party size, the combat-control choices, NPC level-ups). `[Must-match]` (D13)

| Key | Form | Default | Meaning |
| --- | --- | --- | --- |
| `party_member_pid` | Integer, a PID of category 0, 1 or 2 | Required | The member's base prototype. A record whose PID is not a critter's, as the retail file's car trunk is, follows the member, has no control panel and takes no part in combat AI or level-ups (SPEC-019 §3.14) (Revision 2, R13) |
| `area_attack_mode` | List of option words | Empty | Choices the player may set, from the vocabulary below |
| `attack_who` | List | Empty | As above |
| `best_weapon` | List | Empty | As above |
| `chem_use` | List | Empty | As above |
| `distance` | List | Empty | As above |
| `disposition` | List | Empty | As above |
| `run_away_mode` | List | Empty | As above |
| `level_minimum` | Integer 0-99 | 0 | First player level at which the member may level up |
| `level_up_every` | Integer 0-99 | 0 | Player levels between member level-ups; 0 never |
| `level_pids` | List of critter PIDs, or `-1` | Empty | Prototypes for successive member levels; `-1` alone is the empty list, with no diagnostic (Revision 2, R13) |

An option list is the member's **allowed set** for that field (SPEC-019 §2.5).
An empty list offers no choice for that field on SPEC-019's panels. An
unknown option word or an invalid PID in a list is dropped with a diagnostic.
The word `none` in a `disposition` list is ignored, without a diagnostic. When a PID appears in two records, as
`party_member_pid` or in `level_pids`, the higher-numbered record is dropped.
Every one of these cases is a `[Safe rule]`.

**Option vocabularies.** Each word's index is its saved value (section 7.5).
`[Must-match]` (D13)

| Option | Index: word |
| --- | --- |
| `area_attack_mode` | 0 `always`, 1 `sometimes`, 2 `be_sure`, 3 `be_careful`, 4 `be_absolutely_sure` |
| `attack_who` | 0 `whomever_attacking_me`, 1 `strongest`, 2 `weakest`, 3 `whomever`, 4 `closest` |
| `best_weapon` | 0 `no_pref`, 1 `melee`, 2 `melee_over_ranged`, 3 `ranged_over_melee`, 4 `ranged`, 5 `unarmed`, 6 `unarmed_over_thrown`, 7 `random` |
| `chem_use` | 0 `clean`, 1 `stims_when_hurt_little`, 2 `stims_when_hurt_lots`, 3 `sometimes`, 4 `anytime`, 5 `always` |
| `distance` | 0 `stay_close`, 1 `charge`, 2 `snipe`, 3 `on_your_own`, 4 `stay` |
| `disposition` | 0 `custom`, 1 `coward`, 2 `defensive`, 3 `aggressive`, 4 `berserk` |
| `run_away_mode` | 0 `none`, 1 `coward`, 2 `finger_hurts`, 3 `bleeding`, 4 `not_feeling_good`, 5 `tourniquet`, 6 `never` |

An object is a **party candidate** of record N when its PID equals the
record's `party_member_pid` or one of its `level_pids`.

The seven vocabularies are exactly SPEC-019 §2.5's tokens in SPEC-019's
order, so a vocabulary index is also SPEC-019's panel position. (R5)

### 4.6 Line tables

`data/quests.txt`, `data/holodisk.txt`, `data/genrep.txt`,
`data/karmavar.txt` and `data/endgame.txt` use a line grammar, not the
section grammar. All five are optional; an absent file is an empty table.
`[Must-match]` (D14, R6)

- Rules 1 and 2 of section 3.3 apply.
- From the first `#` or `;` to the end of a line is a comment. Lines are
  trimmed, and empty lines skipped.
- Every other line is a List. Each field must have the form the table below
  gives. A line with the wrong field count or an invalid field is dropped,
  with a diagnostic.
- Records keep file order. The record ordinal is the position among the
  valid lines, from 0.

| File | Fields, in order | Consumer |
| --- | --- | --- |
| `quests.txt` | location message, description message, campaign global, display threshold, completed threshold: 5 Integers | Section 5.9 |
| `holodisk.txt` | campaign global, title message, first text message: 3 Integers | Section 5.9 |
| `genrep.txt` | karma threshold, title message: 2 Integers | SPEC-019 §3.9 |
| `karmavar.txt` | campaign global, art, name message, description message: 4 Integers; art is an `intrface.lst` row index, or -1 for none | SPEC-019 §3.9 |
| `endgame.txt` | campaign global, value, art, narration, then optionally direction: 4 or 5 fields. Global and value are Integers. Art is an Integer `intrface.lst` row index or a Stem (`art/intrface/<stem>.frm`). Narration is a Stem (`sound/speech/narrator/<stem>.acm`). Direction is an Integer -1, 0 or 1, default 0 | SPEC-019 §3.20 |

Message sources: quest locations come from `text/<language>/game/map.msg`
and quest descriptions from `quests.msg`; holodisk titles and text from
`pipboy.msg`; reputation titles and karma-variable names and descriptions
from `editor.msg`. All are under `text/<language>/game/`. A missing message
displays SPEC-002 §3.5's placeholder.

- **`genrep.txt` selection.** For karma value k, the title is that of the
  record with the greatest threshold not above k; equal thresholds go to the
  first in file order. No such record means no title.
- **`karmavar.txt`.** A record applies while its global is nonzero.
- **`endgame.txt`.** A record applies when its global equals its value.
  Direction 0 is a still slide; 1 and -1 are the desert pans of SPEC-019
  §3.20. For an art index, the palette is the row's name with `.pal` in
  place of `.frm`; for a stem, `art/intrface/<stem>.pal`.

### 4.7 Files this specification does not define

These Fallout 2 files are outside this specification, and the engine does not
read them under the Fallout 2 profile until another specification defines
them. (D15)

- `data/ai.txt`'s Fallout 2 keys (the seven control keys of section 4.5 inside
  packets). The packets are read as SPEC-009 reads Fallout 1's; the added
  keys are ignored. So SPEC-019 §2.5's second source of initial values never
  applies in this revision. (R6)
- `data/enddeath.txt` (death endings): not shown in this revision.
- Town reputations: Team A knows no Fallout 2 data file for them, so
  SPEC-019 §3.9 draws no town-reputation rows. (R6)
- Progression tables (the level table, the perk table and its numbering):
  until a later specification, SPEC-011's tables and perk indices apply
  provisionally. The saved perk component has 119 words (section 7.3);
  words 64 to 118 are written 0 and ignored on load. (R6)
- `proto/*/*.lst`, `.pro` files and map streams: SPEC-016. `scripts/scripts.lst`
  and `.int`: SPEC-004 and SPEC-017.
- Message files: SPEC-002.

### 4.8 Replacing the compiled campaign tables

Under Fallout 1, `vault_core::tables` holds tables compiled from SPEC-007
and SPEC-008, each replaceable by override text at
`data/vault-engine/<table>.txt`. Under Fallout 2: `[Must-match]` (D15)

| Compiled table | Fallout 2 source | Shape |
| --- | --- | --- |
| `terrain` | Item 1 of each subtile (section 4.4.2) | `6 x ceil(T / H)` rows by `7 x H` columns, not 30 by 28 |
| `frequency` | Items 3-5 of each subtile, resolved to percentages | Three grids (morning, afternoon, night), not one |
| `region` | Item 6 of each subtile, resolved to a table number or none | As `terrain` |
| `towns` | `city.txt` areas | `A` areas, not 12 |
| `hotspots` | `city.txt` entrances | `E` slots per area, at most 32, not 7 |
| `maps` | `maps.txt` records | Numbers 0 to `M_T - 1`, not 66 |
| `critical`, `critical_failure` | The Fallout 1 compiled tables, unchanged | As Fallout 1 |

- The six world tables are built from the files when Play starts or a save
  loads. Their `data/vault-engine/<table>.txt` overrides are **not read**
  under Fallout 2; a present one gets a diagnostic. A mod changes them by
  shipping its own `data/maps.txt`, `data/city.txt` or `data/worldmap.txt`
  in a loose or mod layer, which wins by section 1's lookup order.
- The critical tables have no Fallout 2 source here. The Fallout 1 tables
  are a **provisional** default (D15), overridable as under Fallout 1. Kill
  types 15-18 (section 7.3) use the critical row of kill type 0 until a
  later specification supplies Fallout 2's tables.
- An enhanced-QoL `[[maps]]` registration (roadmap, part 2 d) under Fallout 2
  must use a number of at least `M_T`, instead of 66.

### 4.9 Elevators

Fallout 1's elevator table (SPEC-013 §2.7) is compiled data with Fallout 1
map numbers, so it does not apply under Fallout 2. Team A knows no Fallout 2
data file that holds elevators, nor their destinations. This specification
therefore defines an **engine-owned** elevator table, read from
`data/vault-engine/elevators.txt` through the master database (`data_root`, a
mod layer, `patch000.dat`, `MASTER.DAT`). The engine ships no such file. It is
read under the Fallout 2 profile whatever the preset, because it is the only
elevator source. `[Engine layout]` (R3)

**Grammar.** Section 3.3's grammar. Each `[Elevator N]` section, N from 0 to
99, is elevator type N, the `metarule` rule 15 operand (SPEC-017 §5.1).

| Key | Form | Default | Meaning |
| --- | --- | --- | --- |
| `background` | Stem | Required | `art/intrface/<stem>.frm`, the window background |
| `panel` | Stem, or empty | None | `art/intrface/<stem>.frm`, drawn as SPEC-013 §3.21's panel |
| `stop_K`, K 0-3 | List of 4 items: map number (Integer 0-999, a present `maps.txt` record), elevation (Integer 0-2), hex (Integer 0-39999), key (one printable ASCII character) | None | Button K's destination and key |
| `doors` | List of scenery PIDs (Integer, category 2) | Empty | The doors that SPEC-013 §3.21 steps 3 and 4 close; empty closes none |

- An invalid stop is dropped. The buttons are the valid stops in ascending
  K, renumbered from 0. A record with fewer than 2 or more than 4 valid
  stops is dropped, with a diagnostic. `[Safe rule]`
- An invalid PID in `doors` is dropped, with a diagnostic.

**Behavior.** As SPEC-013 §3.21, with this table in place of §2.7: the
window, buttons (`ebut_out.frm` and `ebut_in.frm`), gauge (`gaj000.frm`),
sounds, animation and the performing of the result, with the record's
`doors` in place of the three Fallout 1 door PIDs. Under Fallout 2, all four
of §3.21's `[May-improve]` safe rules apply: lowercase keys are accepted too,
the exact target cell is drawn at the end, the start level is the stop that
matches both the current map and the player's elevation (else 0), and
Cancel changes nothing. `[Must-match]` (R3)

**A type without a record** (including every type when the file is absent)
makes rule 15 do nothing, with one diagnostic, as SPEC-013 §3.21 treats a
type outside 0-11. Level changes that scripts make by other means, such as
exit grids, `load_map` and object moves, are unaffected. `[Safe rule]` (R3)

## 5. World-map and map rules (deltas from SPEC-008 §3)

### 5.1 Map numbers and names (SPEC-008 §3.10)

`[Must-match]` (D8)

- A **map number** is a `maps.txt` record number. It replaces "message
  number 0-65 of `map.msg`" everywhere in SPEC-008 §3.10, SPEC-014 §3.35 and
  SPEC-013 §3.22: in the automap index, the excluded set, the town (area)
  lookup and city music.
- A record's **match text** is its stem, upper-cased, followed by `.MAP`.
- **Assigning the current map number:** as SPEC-008 §3.10, except that step
  2 compares with the match texts of the records in ascending number, and
  the first equal one gives the number.
- **Matching a map name** (used by `load_map` with a string, SPEC-017):
  uppercase the string and replace its first `.SAV` with `.MAP`, then compare
  it exactly with the match texts in ascending number. So `artemple.map`,
  `ARTEMPLE.MAP` and `artemple.sav` match `ARTEMPLE.MAP`; `ARTEMPLE` and
  `MAPS\ARTEMPLE.MAP` match nothing. A string of 16 bytes or more matches
  nothing. `[Safe rule]`
- **The `load_map` entrance parameter.** Under Fallout 2 it is popped and
  **written nowhere**: no campaign global, including Fallout 1's element 32,
  receives it. The map loads at its header's default entrance, and the
  scripts place the player. The same holds for every other path that
  Fallout 1 makes write campaign elements 32 or 65 (SPEC-008 §3.5, SPEC-014
  §3.39). A later revision may name a global once the SPEC-017 §8 audit
  gives evidence for one. `[Must-match]` (R1)
- **Loading by number** opens `maps/<stem>.map`, probed for its working
  `.SAV` first, as SPEC-008 §3.10 "Working `.SAV` names". An absent number
  loads nothing, with a diagnostic. `[Safe rule]`
- Working `.SAV` names, the stem rule and the `SAVE.DAT` file name field are
  as SPEC-008 §3.10.

### 5.2 Area lookup, music, exclusion, rest and aging

`[Must-match]` (D16, D22)

- **Area of a map.** A map's area is the lowest-numbered valid area that has
  a valid entrance naming the map's lookup name, in any slot. A map that no
  entrance names, and map number -1, have area -1. The area takes the place
  of SPEC-008 §3.10's town in its three uses: script-directed travel after a
  map change (section 5.4), rest permission (below), and town-map and
  world-map entry (SPEC-019).
- **City music.** As SPEC-008 §3.10, with the record's `music` as the city
  track. A record without `music` behaves as map number -1 does there:
  nothing replaces the background track. The background track and the
  music on/off state are as SPEC-008 §3.10. SPEC-017's `set_map_music`
  override (SPEC-017 §4.4) takes precedence while it lasts.
- **World-map music.** As SPEC-008 §3.10, `03wrldmp`, unless SPEC-017's
  `metarule3` 102 override is in force. (R6)
- **Display names.** An area's display name is its `area_name`. A map's
  **location name** (the save screen's location line, SPEC-019 §3.11) is the
  display name of its area; for area -1, the map's `lookup_name`; without
  either, empty. There are no elevation names under Fallout 2. (R6)
- **Map known and visited flags.** Each map number has a known flag and a
  visited flag, saved in section 7.4. A successful load of a map sets both.
  SPEC-017's `mark_area_known` with mark type 1 sets or clears them, and
  `metarule` rule 19 reads the known flag. At a new game all are clear.
  (R6)
- **Excluded set.** A map is excluded when its record has `saved=No`, or the
  number is absent or -1. Leaving an excluded map writes no working `.SAV`
  and deletes any that exists, as SPEC-014 §3.35 step 5. The SPEC-014 §3.35
  list of Fallout 1 map numbers does not apply.
- **Aging.** SPEC-014 §3.35's revisit rules apply, except that rule 3, the
  replacement of corpses with remains, is skipped on a map with
  `dead_bodies_age=No`. Rule 3's special case (critter `0x01000031` and
  item 213) is Fallout 1 data and is not used under Fallout 2: every corpse
  takes the general branch. That branch's remains prototype, miscellaneous
  PID `0x05000004`, is kept provisionally. When it does not load, the corpse
  stays as it is and drops nothing. `[Safe rule]` (D22)
- **Rest.** SPEC-008 §3.9's alarm menu, time advance and healing are
  unchanged. Permission replaces its fixed map and town lists:
  1. If `can_rest_here` is No for the player's elevation, resting is
     refused.
  2. Otherwise SPEC-008 §3.9's last-attacker test applies: a living
     non-player critter on the elevation whose last attacker is the player
     refuses rest.
  3. The team test of SPEC-008 §3.9 applies only on maps whose area is -1.

### 5.3 World grid geometry (SPEC-008 §3.4 and §3.6)

`[Must-match]` (D10)

- The grid has `C = 7 x H` columns and `R = 6 x ceil(T / H)` rows of 50 by
  50 subtiles. The world measures `50 x C` by `50 x R` pixels. X grows
  rightward and Y downward, as SPEC-008 §3.4. (With `H` = 4 and `T` = 20,
  this is SPEC-008's 1400 by 1500 world and 28 by 30 grid.)
- World pixel (x, y) lies in grid cell `(floor(x / 50), floor(y / 50))`.
  Cell (c, r) is subtile key `(c mod 7)_(r mod 6)` of tile
  `floor(r / 6) x H + floor(c / 7)`. A cell of a tile number at or beyond
  `T` takes the subtile defaults.
- The fixed grids of SPEC-008 §3.6 and the lookup grid size 30 by 28 do not
  apply. The viewport and its scroll limits are SPEC-019 §3.12.1's.

### 5.4 Travel (SPEC-008 §3.4)

`[Must-match]` unless tagged. (D17)

- The movement line rule, `W`, `Q`, the time charge, queue processing during
  travel and the 24-hour healing every `W` iterations are as SPEC-008 §3.4.
- **Terrain schedule.** It replaces SPEC-008 §3.4's desert, mountain and
  city rows. Let d be the difficulty of the current subtile's terrain. The
  terrain counter starts at 0 for each trip. On each iteration: charge `Q`;
  subtract 1 from the counter; if the counter is then 0 or less, attempt one
  pixel step and set the counter to d. So difficulty 1 steps every
  iteration, and difficulty 2 every other iteration, starting with the
  first.
- **Walk mask.** After a proposed step to world pixel (x, y), with x and y
  inside the world: take the tile containing it. If that tile has a mask
  (section 4.4.2), test tile-local pixel `(x mod 350, y mod 300)`. A set bit
  blocks the step: the prior position is restored and movement stops, as
  SPEC-008 §3.4. A step that would leave the world is blocked the same way.
  SPEC-008's untested row 0 and columns from 1064 do not carry over.
  `[Safe rule]`
- **Script-directed travel** (SPEC-008 §3.4, run after each map load) is
  replaced by a direct move, as SPEC-019 §3.12.5 says: after a map loads
  whose area is not -1, the world position becomes that area's `world_pos`
  at once. There is no travel simulation, no time charge, no healing, no
  jitter draw and no discovery change. Area -1 leaves the position
  unchanged. `[Must-match]` (R6)

### 5.5 Discovery and fill (SPEC-008 §3.5)

`[Must-match]` (D18)

- The saved discovery grid has exactly R rows of C bytes (section 7.4), with
  SPEC-008's values 0 unknown, 1 seen and 2 visited. The 31 by 29 array of
  SPEC-008 does not apply.
- The every-third-iteration reveal (3 by 3, or 5 by 5 with Scout, clipped to
  the grid, never lowering a 2) is as SPEC-008 §3.5.
- The fixed west/ocean threshold table is replaced by **fill**. When the
  every-third-iteration reveal sets the current cell to 2, and that cell's
  fill is not `No_Fill`, every cell from it to the grid edge in each named
  direction, on the same row (`_W`, `_E`) or the same column (`_N`, `_S`),
  becomes 2. The cell itself is not counted. A two-letter direction does
  both of its parts, for example `Fill_NW` fills north and west.
- A new-game reset clears all R by C bytes.

### 5.6 Encounter check and selection (SPEC-008 §3.5)

`[Must-match]` (D19, D20). At each check of SPEC-008 §3.4 (every `W`
iterations):

1. **Period.** The period of the game time-of-day: morning 06:00-11:59,
   afternoon 12:00-17:59, night 18:00-05:59.
2. **Chance.** Let p be the percentage of the current cell's frequency for
   that period. Draw `random(1, 100)`. This draw is always made, even when
   step 3 then suppresses the encounter, as SPEC-008's 3d6 is.
3. **Suppression.** There is no encounter when the draw exceeds p, when the
   cell has no encounter table, or when the party is inside an area by
   SPEC-019 §3.12.2's containment rule (the circle of the area's `size`
   radius, SPEC-019 §2.6), whether or not the area is known. (R6)
4. **Eligible entries.** In ascending entry number: an entry is eligible
   when its counter is not 0 and its condition holds. Conditions are
   evaluated in that order, each once.
5. **Special gate.** If any eligible entry is Special, draw three
   `random(1, 6)` and compute
   `sum - 5 + effective Luck + 2 x Explorer perk ranks` (SPEC-008 §3.5's
   special test). Below 18, Special entries become ineligible. No draw is
   made when no eligible entry is Special.
6. **Pick.** Let S be the sum of the eligible entries' Chance weights. S of
   0 means no encounter. Otherwise draw `random(1, S)` and take the first
   entry, in ascending number, at which the running sum of weights reaches
   the draw.
7. **Counter.** A positive counter decreases by 1.
8. **Map.** The entry's `Map:` if any; otherwise one of the table's `maps`,
   drawn with `random(1, n)`; otherwise one of the random map list of the
   cell's terrain, drawn the same way. With none of these, there is no
   encounter, with a diagnostic. `[Safe rule]`

The SPEC-008 §3.5 3d6 frequency table, special-map table, terrain map
choices and the writes of campaign globals 32 and 65 do not apply under
Fallout 2. The encounter flash (ordinary, or special for a Special entry)
is SPEC-019 §3.12.7's. Neither SPEC-019 nor this specification lets an
encounter be avoided, so every selected encounter is entered. (R6)

### 5.7 Entering an encounter and spawning

`[Must-match]` (D21)

**Dropping to terrain** (SPEC-019 §3.12.4: a click on the party outside
every area). The map is one of the random map list of the party cell's
terrain (section 4.4.3), drawn with `random(1, n)` (no draw when n is 1). An
empty list loads nothing, and the world map stays open. The entrance is
chosen by step 1 below. No group is spawned and no counter changes. (R6)

1. **Entrance.** When the map has start points, draw `random(0, k - 1)`
   among its k points. The entrance is that point's hex and elevation, with
   the header's orientation. Without start points, it is the header's
   default entrance. The map then loads through SPEC-006 §3.5.
2. **Groups.** After the load completes, each group of the entry's
   expression is spawned in expression order. A group with an unknown name
   is skipped. For each group:
   1. Draw the count `n = random(min, max)`.
   2. Draw the direction `a = random(0, 5)`.
   3. The eligible types are those, in ascending type number, whose
      condition holds; each condition is evaluated once per group. No
      eligible type skips the group.
   4. For each critter k from 0 to n - 1, choose a type. With one eligible
      type, take it, with no draw. Otherwise each type's weight is its
      Ratio; a type without Ratio weighs 0 when any eligible type has one,
      and when none has one every type weighs 1. With total weight 0, every
      type weighs 1. Draw `random(1, total)` and take the first type at
      which the running sum reaches the draw.
   5. **Placement.** From the player's hex, step
      `distance + spacing x floor(k / 6)` hexes in direction
      `(a + k) mod 6` (SPEC-003 §3.4 edge stepping, stopping at the grid
      edge). Then search outward, ring by ring up to radius 5, for the first
      hex on the player's elevation that no blocking object occupies. Rings
      are walked in SPEC-003's neighbor order. When no hex is free, the
      critter is not created, with a diagnostic. `[Safe rule]`
   6. **Creation.** In step 5, `distance` is the type's own `Distance` when
      it has one, else the group's (Revision 2). Create the critter from the
      type's PID at that hex, facing the player. An item or scenery type
      creates that object at the hex, with nothing else applied (Revision 2). Add each `Item:` stack, drawing each count with
      `random(min, max)` in field order. Make a `(wielded)` item its held
      weapon. Attach the type's script, if given, as a new critter-category
      instance. A `Dead` critter is then put into the dead state that
      SPEC-007 gives a critter killed by normal damage, with no animation,
      sound or experience award.
3. **Relations.** `AND`, `FIGHTING` and `AMBUSH Player` are parsed and
   exposed, but have no engine effect in this revision (SPEC-019 defines
   none): every group keeps its prototype team, and no combat is started by
   the engine. Scripts may still start combat. (D21)

### 5.8 Areas and entrances (SPEC-008 §3.7)

`[Must-match]` (D9, D16)

- Each area has three saved states (section 7.4): known (0 unknown, 1 known,
  2 visited), locked (0 or 1), and per entrance slot known (0 or 1). A new
  game sets known to 1 for `start_state=On` areas, locked from `lock_state`,
  and each entrance's known value from its first item.
- A known, unlocked area can be travelled to and entered. A locked area is
  drawn but cannot be entered. On first entry to an area whose state is 1,
  the state becomes 2 and its lowest-numbered known entrance is loaded
  directly; later entries show the town map. These rules replace SPEC-008
  §3.7's campaign-global tests, its Cathedral, Military Base and Vault 13
  substitutions, and its fixed hotspot catalog. The screens, buttons and
  labels are SPEC-019 §3.12-§3.13's.
- The known, locked and entrance states change only through scripts
  (SPEC-017) and the first-entry rule above. Campaign globals do not drive
  them under Fallout 2.
- **Area positions.** Each area's current world position starts as its
  `world_pos` and is saved (section 7.4). SPEC-017's `wm_area_set_pos`
  changes it. Every rule here and in SPEC-019 that uses an area's position
  uses the current one. (R6)
- **Entrances by map** (SPEC-017's `metarule3` 104, with map m and elevation
  e). The affected slots are every valid entrance slot, in every area, whose
  destination is map m and where e is -1, or the entrance's item 5 equals e,
  or item 5 is -1. (R6)

### 5.9 Pip-Boy quests and holodisks (SPEC-008 §3.8)

`[Must-match]` (D14)

- **Quests.** For a record with global value v, display threshold d and
  completed threshold c: hidden when `v < d`; otherwise completed when
  `v >= c`; otherwise active. How active and completed quests are drawn is
  SPEC-019 §3.10's.
- **Groups.** Quests are grouped by location message, in order of the
  group's first appearance in the file, and keep file order inside a group.
  A group is listed when at least one of its quests is not hidden. The fixed
  Fallout 1 quest table and the water-chip special case do not apply.
- **Holodisks.** Holodisk ordinal i is record i. It is acquired when its
  global's value is **positive**: this applies SPEC-008 §3.8's safe rule in
  every pass. The title is the title message. The content window is the 500
  ids from the first text message, with SPEC-008 §3.8's paging rules,
  `**END-DISK**`, `**END-PAR**`, the 35-record page, and its `[May-improve]`
  safe rules (last-page index and stopping at the window).
- The Pip-Boy component of `SAVE.DAT` still has zero bytes.

## 6. New-game flow data

These are the data an implementer needs so that **Play** starts a Fallout 2
game. The screens and their order belong to SPEC-019; where SPEC-019 is
silent, SPEC-014's Fallout 1 flow applies with the data below. (D23-D27)

### 6.1 Clock and calendar

- A game reset sets the clock to tick **264600**, 07:21:00 of the first game
  day, as SPEC-014 §3.1 step 8. `[Must-match]` (D24)
- The calendar is SPEC-005 §3.5's fixed non-leap calendar, but tick 0 is
  00:00 on **25 July 2241**. Game day `trunc(ticks / 864000)` counts forward
  from that date. So tick 264600 is 25 July 2241 at 07:21, and the date
  fields of the `SAVE.DAT` header, `get_month`, `get_day` and every displayed
  date use this calendar. `[Must-match]` (D24)

### 6.2 Campaign and map globals

- The campaign initializer is `data/vault13.gam`, with section marker
  `GAME_GLOBAL_VARS`. Its grammar is as SPEC-006 §2.1; its count G is whatever
  the file allocates. Map globals come from `maps/<stem>.gam`, as SPEC-006
  §2.1. `[Must-match]`
- Under Fallout 2, **no campaign global has a fixed engine meaning** from
  this specification. These Fallout 1 uses are switched off:
  - the water countdown and its movies (globals 10 and 101; SPEC-012 §3.12):
    the midnight event does only lock unjamming and its other non-water
    work;
  - the world-map globals of SPEC-008 §3.5 and §3.7 (17, 18, 32, 65, 67-78
    and 558-600);
  - the fixed Pip-Boy tables (SPEC-008 §3.8), replaced by section 5.9.

  The engine writes no campaign global under Fallout 2. In particular,
  `load_map`'s entrance parameter is stored nowhere (section 5.1).
  `[Must-match]` (D27, R1)

### 6.3 Premade and new characters

- The premade sheets and bios are the same files as Fallout 1, in the same
  cyclic order: `premade/combat.gcd` and `.bio`, `premade/stealth.gcd` and
  `.bio`, `premade/diplomat.gcd` and `.bio`. Portraits, ID text and the
  screen are SPEC-019 §3.3's. (D25)
- A `.gcd` sheet is the 428-byte layout of SPEC-006 §2.2, with its loading
  rules and safe rule (SPEC-006 §2.3), unchanged. The trait indices keep
  their positions 0-15; their names come from `trait.msg`. Index 13 is
  Sex Appeal under Fallout 2, not Night Person. Trait and perk effects
  under Fallout 2 belong to a later specification. `[Must-match]` (D25)
- "Create character" starts from the same blank defaults as SPEC-014 §3.7.
- Initial player state is as SPEC-014 §3.12: level 1, experience 0, no
  unspent skill points, reputation and karma 0, HP at maximum, and no
  starting items from the engine. Scripts grant any kit.

### 6.4 Movies

The Fallout 2 game movie table replaces SPEC-012 §2.11's. Index m is the
`play_gmovie` operand and the position in the played-movie component (section
7.3). Files are under `art/cuts/`. `[Must-match]` (D26)

| Index | File | Index | File | Index | File |
| ---: | --- | ---: | --- | ---: | --- |
| 0 | `iplogo.mve` | 6 | `car.mve` | 12 | `artimer1.mve` |
| 1 | `intro.mve` | 7 | `cartucci.mve` | 13 | `artimer2.mve` |
| 2 | `elder.mve` | 8 | `timeout.mve` | 14 | `artimer3.mve` |
| 3 | `vsuit.mve` | 9 | `tanker.mve` | 15 | `artimer4.mve` |
| 4 | `afailed.mve` | 10 | `enclave.mve` | 16 | `credits.mve` |
| 5 | `adestroy.mve` | 11 | `derrick.mve` | | |

- Startup plays `iplogo.mve` and then `intro.mve`, as SPEC-014 §3.1 step 9.
- The new-game hand-off plays `elder.mve` (index 2) in place of
  `ovrintro.mve`, with SPEC-014 §3.12 step 2's flag `0x04`.
- The `play_gmovie` flags per index and every other caller are SPEC-019
  §3.21's, which lists the same 17 files in the same order. (R5)
  A missing file fails the play as SPEC-012 §3.12 step 1.

### 6.5 Starting map and world state

`[Must-match]` (D23)

- The new-game map is `maps/artemple.map`, loaded by that fixed name, with
  the **header's default entrance** (hex, elevation and orientation, SPEC-016
  over SPEC-003 §2.7). Its map number is assigned by section 5.1; when no
  record matches it is -1, with a diagnostic, and the game still starts.
- **Start area.** The start area is the area of `maps/artemple.map` (section
  5.2). When that is -1, it is area 0; when area 0 is absent too, it is the
  lowest valid area.
- **World state at a new game:**
  - world position: the start area's `world_pos`;
  - current area: the start area, with state 2 (visited); current entrance:
    the lowest-numbered slot naming the starting map, else -1;
  - area, lock and entrance states from `city.txt` (section 5.8);
  - discovery: all 0, then each known area's cell 1, then the start area's
    cell 2;
  - area positions: each area's `world_pos`;
  - map known and visited flags: all clear, until the starting map's load
    sets its own (section 5.2);
  - encounter counters: each entry's `Counter`;
  - car: `has_car` 0, `car_area` -1, `car_map` -1, `car_fuel` 0 (SPEC-019
    §2.4);
  - party: the player alone, with no retained control records;
  - played-movie record: all 17 bytes 0.
- City music is the starting map's `music` (section 5.2), in place of
  SPEC-014 §3.12 step 8's `06vault`.

### 6.6 The hand-off sequence (SPEC-014 §3.12)

`[Must-match]` (D23)

1. **Before the selection screen**, parse the section 4 tables and the
   campaign initializer, and build the automap database (section 7.6). A
   refusal returns to the main menu with a message naming the file and
   line. Nothing has changed then.
2. Then follow SPEC-014 §3.12 steps 1 to 10, with three changes: step 2
   plays `elder.mve`; step 7 loads `maps/artemple.map` as in section 6.5;
   step 8 starts that map's city music.
3. The world state of section 6.5 is installed after the reset and before
   step 7's map load. The map load's direct move (section 5.4) then leaves
   the party at the start area's position.
4. While Fallout 2 scripts are gated (section 6.7), the hand-off is
   otherwise unchanged. When it completes, the message monitor shows the
   gate notice.

### 6.7 Play while scripts are gated

**Revision 2: the gate is open.** SPEC-017 Revision 3 records the retail
audit and opens the gate, so Fallout 2 Play runs scripts. The rules of this
section no longer apply. A save that carries the gated mark is refused, as
section 7.2 says. The text below is kept to define that mark.

SPEC-017 §8.5 kept Fallout 2 **script execution** gated until its corpus
audit was recorded. Play itself is not gated: SPEC-017 §8.7 defines what a
Play session does meanwhile, and this specification, SPEC-017 and SPEC-019
all follow it. In short (SPEC-017 §8.7 is the full rule): `[Must-match]` (R4)

- the launcher's Play is available under `game = "fallout2"` once section 2
  accepts the install;
- the new game starts as section 6.6 says, and maps load, render and can be
  played: movement, looking, the interface, the world map, encounters,
  combat the player starts, save and load;
- no `.INT` is loaded and no script procedure runs; script instance records
  are still read, kept and saved;
- after each completed new-game hand-off and each successful game load, the
  message monitor prints SPEC-017 §8.7's one-line notice;
- a save made while scripts are gated is marked so in its header (section
  7.2).

## 7. Saves (deltas from SPEC-008 §2.4-§3.3)

Interoperability with save slots written by the original Fallout 2 executable
is **not claimed**. The layout below is the Fallout 1 layout with the deltas
that the Fallout 2 data needs. The engine writes and reads exactly this
layout under the Fallout 2 profile, and refuses any other. (D28)

### 7.1 Slot files

As SPEC-008 §2.4: ten slots `SAVEGAME/SLOT01` to `SLOT10`, each with
`SAVE.DAT`, the listed working `.SAV` maps and `AUTOMAP.SAV`, and the same
`.BAK` backups. `[Must-match]`

### 7.2 `SAVE.DAT` header

The 30051-byte header of SPEC-008 §2.4, with these differences:

| Offset | Bytes | Fallout 2 value | Tag |
| --- | ---: | --- | --- |
| `0x0018` | 2 | Version major 1 | `[Must-match]` |
| `0x001A` | 2 | Version minor **2** (Fallout 1 writes 1) | `[Must-match]` (D28) |
| `0x0065`-`0x0069` | 6 | Game month, day and year from section 6.1's calendar | `[Must-match]` |
| `0x0071` | 2 | Current map number from section 5.1, or -1 as `0xFFFF` | `[Must-match]` |
| `0x74E3` | 16 | ASCII `VAULTENG F2SAVE1`, the layout tag | `[Engine layout]` (D28) |
| `0x74F3` | 4 | G, the campaign-global count, unsigned 32-bit | `[Engine layout]` (D28) |
| `0x74F7` | 1 | Gated mark: 1 when the save was made while Fallout 2 scripts were gated (section 6.7), else 0 | `[Engine layout]` (R4) |
| `0x74F8` | 107 | Zero | `[Engine layout]` |

Every other field, including the signature, the thumbnail and the
character-name, description and file-name fields, is as SPEC-008 §2.4.

**Reading a header** under Fallout 2. Each check fails the slot. The slot
list then shows the "old version" state of SPEC-014 §3.14, as SPEC-019 §3.11
says, and a load is refused before any working file changes.
`[Safe rule]` (D28)

1. The full 30051 bytes are present, and the first 18 signature bytes match
   (SPEC-008 §2.4).
2. The version is exactly 1.2. So a Fallout 1 slot (1.1) never loads.
3. The 16 tag bytes match.
4. The stored G equals the G of the installed `data/vault13.gam`. A slot
   saved with a different campaign initializer cannot be parsed (SPEC-008
   §2.5), so it is refused rather than misread.
5. The gated mark is 0 or 1. A slot marked 1 loads only while scripts are
   still gated. Once the gate is open, it fails: its maps never ran their
   scripts, so it would continue in a state no script expects. A slot
   marked 0 loads under either state; while gated, its scripts simply do
   not run. (R4)

The remaining 107 reserved bytes are not checked.

### 7.3 Ordered payloads

The 27 positions of SPEC-008 §2.5 keep their order, their zero-byte slots and
their lack of framing, except for the four counts and three layouts marked
"changed" below. `[Must-match]` (D29) for the counts.

| Order | Component | Fallout 2 |
| ---: | --- | --- |
| 0-1 | Preparation, player combat identity | As SPEC-008 |
| 2 | Campaign snapshot A | `4 x G + 1` bytes, as SPEC-008. The final byte is written 0 and ignored on load, since Fallout 2 has no water countdown (D29) |
| 3 | Map manifest | As SPEC-008, including the 12-byte name limit |
| 4 | Campaign snapshot B | As order 2 |
| 5 | Player object and inventory | As SPEC-008, with the SPEC-016 object records |
| 6 | Player critter statistics | As SPEC-008: one 32-bit word and the 368-byte SPEC-006 prefix, 372 bytes |
| 7 | Kill counters | **Changed:** 19 words, 76 bytes. Kill types 15-18 are gecko, alien, giant ant and big bad boss, after SPEC-003 §2.4's 0-14 (D29) |
| 8-9 | Tagged skills, random generator | As SPEC-008 |
| 10 | Perk ranks | **Changed:** 119 words, 476 bytes, player only. While SPEC-011's perk table applies provisionally (section 4.7), words 64-118 are written 0 and ignored on load (D29, R6) |
| 11-14 | Combat, combat AI, progression, item subsystem | As SPEC-008 |
| 15 | Timed queue | As SPEC-008 §2.6, event types 0-12 only. A record of another type refuses the load (D29) |
| 16-19 | Traits, automap options, preferences, editor | As SPEC-008 |
| 20 | World-map state | **Changed:** section 7.4 |
| 21 | Pip-Boy | Zero bytes, as SPEC-008 |
| 22 | Played-movie flags | **Changed:** 17 bytes, one per section 6.4 index |
| 23 | Skill-use timestamps | As SPEC-008 |
| 24 | Party membership | **Changed:** section 7.5 |
| 25-26 | Interface state, finalization | As SPEC-008 |

### 7.4 World-map component (order 20)

`[Engine layout]` (D30). All words big-endian. Offsets are relative to the
component's start.

| Offset | Bytes | Field |
| --- | ---: | --- |
| `+0` | 4 | L, unsigned: the byte length of everything after this word |
| `+4` | 4 | World X, signed |
| `+8` | 4 | World Y, signed |
| `+12` | 4 | Current area, signed; -1 none |
| `+16` | 4 | Current entrance slot, signed; -1 none |
| `+20` | 4 | `has_car`, signed: 0 or 1 (SPEC-019 §2.4) |
| `+24` | 4 | `car_area`, signed: an area, or -1 |
| `+28` | 4 | `car_map`, signed: a map number, or -1 |
| `+32` | 4 | `car_fuel`, signed: 0 to 80000 |
| `+36` | 2 | C, unsigned 16-bit: grid columns |
| `+38` | 2 | R, unsigned 16-bit: grid rows |
| `+40` | `C x R` | Discovery bytes, row-major, values 0-2 |
| next | 4 | A, unsigned: area count |
| next | per area | For each area 0 to A - 1: one known byte (0-2), one locked byte (0-1), the area's current world X and Y (two signed words, section 5.8), one entrance count byte E, then E entrance-known bytes (0-1) |
| next | 4 | M, unsigned: map-flag count, equal to `M_T` |
| next | M | One byte per map number 0 to M - 1: bit `0x01` known, bit `0x02` visited, other bits 0 (section 5.2) |
| next | 4 | K, unsigned: counter record count |
| next | `12 x K` | K records of three signed words: table number, entry number, remaining count |

These are SPEC-019 §2.4's four car values (R2, R5). SPEC-019's town-list
scroll offset is not saved: it is 0 after every load.

The writer:
- writes absent areas, absent entrance slots and absent map numbers as zero
  bytes, so that A, each E and M always equal the installed tables' values;
- writes one counter record for each entry whose `Counter` is not -1, in
  ascending table and then entry number;
- writes nothing about travel in progress, which, as in SPEC-008 §3.4, is not
  saved.

**Reading.** `[Safe rule]` (D30)

- L must equal the bytes the fields after it actually consume. C and R must
  equal the installed grid; A and every E must equal the installed `city.txt`;
  M must equal `M_T`. Any mismatch, a byte outside its range, a `has_car`
  other than 0 or 1, a `car_fuel` outside 0-80000, or a current area,
  entrance, `car_area` or `car_map` outside the installed tables (other than
  -1) refuses the load, as a slot from different installed data.
- An area position outside the world is replaced by the area's `world_pos`,
  with a diagnostic.
- A counter record naming no installed limited entry is ignored, with a
  diagnostic. A limited entry with no record keeps its initial `Counter`.
  A remaining count outside -1 to 1000 refuses the load.
- World X and Y outside the world are clamped into it, with a diagnostic.

### 7.5 Party component (order 24)

`[Engine layout]` (D31, R5). It stores SPEC-019 §2.4's party list, control
records and level stages. It begins as SPEC-008 §2.5 order 24: the 32-bit
party count P (the player included), the 32-bit next-item identity counter,
then `max(P - 1, 0)` 32-bit object identifiers, in party-list order. Then
come:

1. for each of those P - 1 members, in the same order, one **member record**
   of nine signed big-endian words, 36 bytes;
2. a 32-bit count Q of **retained records**: the control records that
   SPEC-019 §3.14 keeps for objects that left the party with `party_remove`;
3. Q retained records, each the object identifier followed by a member
   record, 40 bytes, in ascending identifier order.

| Word | Member record field |
| ---: | --- |
| 0 | `party.txt` record number, or -1 when the member has no control record (SPEC-019 §2.5) |
| 1 | Level stage (SPEC-019 §3.17), 0 to the record's `level_pids` count |
| 2-8 | `disposition`, `area_attack_mode`, `run_away_mode`, `best_weapon`, `distance`, `attack_who`, `chem_use`: each a vocabulary index of section 4.5, in SPEC-019 §2.5's field order |

A member without a control record has words 1-8 written as -1, and they are
ignored on load.

**Reading.** `[Safe rule]`
- P must be from 1 to 32 (SPEC-019 §2.6's capacity), and Q at most 4096.
  Otherwise the load is refused.
- A record number outside the installed records refuses the load. So does a
  level stage outside its range, for a member that has a control record.
- A field value outside its vocabulary is read as that field's neutral value
  (SPEC-019 §2.5), with a diagnostic.

### 7.6 Automap database

As SPEC-008 §2.8, except the index. `[Engine layout]` (D32)

- The index has **1000** map numbers by 3 elevations: 3000 signed words at
  `0x005`, so entries start at `0x2EE5` and the empty database is **12005**
  bytes. The word for map M and elevation E is at `5 + 4 x (3 x M + E)`.
- The initial index is -1 for every elevation of every excluded number
  (section 5.2, including absent numbers 0-999), and 0 otherwise. The fixed
  SPEC-008 initial set does not apply.
- Version 1, the entry format, the 10000-byte raster and its cell writer,
  capture and display are as SPEC-008 §2.8, with its safe rules.
- A database whose length is not at least 12005 bytes, or whose index holds
  an offset inside the index or past the database end, refuses the load.
  `[Safe rule]`

### 7.7 Area-map `.SAV` files

As SPEC-008 §2.9, with the SPEC-016 map stream (version 20, laid out as
version 19) in place of SPEC-003's version 19. The header's map-number word
holds the section 5.1 number. `[Must-match]` (D2)

### 7.8 Saving and loading

As SPEC-008 §3.3 and SPEC-014 §3.35. In addition: `[Safe rule]` (D28)

- A Fallout 2 load validates the header (section 7.2) and parses the whole
  central stream, in memory, before it changes any working file. This
  applies SPEC-008 §3.3's `[May-improve]` rule as the required behavior
  under this profile.
- A save writes the tag and G (section 7.2) and the counts of the installed
  tables.
- A slot whose companions are missing or short is refused as SPEC-008 §3.3
  describes, before any working file changes.

## 8. Test vectors

All data below is synthetic. Integers in hex dumps are big-endian.

### 8.1 Install and mounting

| Install folder entries | Result |
| --- | --- |
| `MASTER.DAT`, `CRITTER.DAT`, required members present | Valid; no patch |
| `master.dat`, `Critter.Dat`, `PATCH000.DAT` | Valid; `PATCH000.DAT` mounted above `master.dat` |
| `MASTER.DAT`, `CRITTER.DAT`, `patch000.dat`, `PATCH000.DAT` | Refused: two entries match `patch000.dat` |
| `MASTER.DAT`, `CRITTER.DAT`, a folder named `patch000.dat` | Valid; no patch |
| `MASTER.DAT`, `CRITTER.DAT`, `patch001.dat` | Valid; `patch001.dat` not mounted |
| `MASTER.DAT`, `CRITTER.DAT`, a `patch000.dat` with a bad footer | Refused, naming `patch000.dat` and the SPEC-015 §2 rule |
| `MASTER.DAT` without `data/party.txt` anywhere in the master database | Refused, naming `data/party.txt` |
| `MASTER.DAT` without `data/party.txt`, but `data_root` holds it | Valid |
| `CRITTER.DAT` missing | Refused, naming `CRITTER.DAT` |

Overlay. `MASTER.DAT` holds `data/maps.txt` with content `B` and
`patch000.dat` holds it with content `A`: the read gives `A`. With
`data_root/data/maps.txt` holding `C` as well: `C`. A member
`art/critters/test.frm` present only in `patch000.dat` is **not found**,
because the critter database has no patch (section 1).

### 8.2 Text grammar

```text
; header comment
stray line
[Map 001]
lookup_name = Test Village   ; trailing comment
map_name=TESTVIL.MAP
saved=No
SAVED=yes
bogus_key=1
[map 1]
music=05raider
```

Expected: one record, map 1. `lookup_name` is `Test Village`. The stem is
`TESTVIL`, so the file is `maps/testvil.map` and the match text
`TESTVIL.MAP`. `saved` is Yes: the later key wins, and keys fold case.
`music` is `05raider`, because `[map 1]` is the same section. Diagnostics:
the stray line, and the repeated `saved`. `bogus_key` has no effect.

### 8.3 `maps.txt`

```text
[Map 000]
lookup_name=Desert Encounter 1
map_name=desert1
music=07desert
saved=No
random_start_point_0=elev:0, tile_num:19086
random_start_point_1=elev:3, tile_num:100
[Map 001]
lookup_name=Test Village
map_name=testvil
music=05raider
[Map 002]
lookup_name=Temple
map_name=artemple
can_rest_here=No,No,Yes
pipboy_active=No
dead_bodies_age=No
[Map 003]
lookup_name=Broken
map_name=toolongname
```

| Map | Stem | Music | Excluded | Rest by elevation | Pip-Boy | Aging | Start points |
| ---: | --- | --- | --- | --- | --- | --- | --- |
| 0 | `desert1` | `07desert` | Yes | Yes, Yes, Yes | Yes | Yes | One: elevation 0, hex 19086 (point 1 dropped, elevation 3) |
| 1 | `testvil` | `05raider` | No | Yes, Yes, Yes | Yes | Yes | None |
| 2 | `artemple` | None | No | No, No, Yes | No | No | None |

Map 3 is dropped: its stem has 11 bytes. `M_T` is 3. Name matching:
`ARTEMPLE.MAP` and `artemple.sav` give 2; `ARTEMPLE` gives nothing.
Leaving map 0 writes no `DESERT1.SAV` and deletes any that exists.

Current-map assignment. A stream opened as `MAPS\ARTEMPLE.MAP` with header
name `artemple.map` and header map-number word 7 gets map number 2, by
name, and its working file is `ARTEMPLE.SAV`. A save made there stores
`ARTEMPLE.sav` at `0x0073` (`41 52 54 45 4D 50 4C 45 2E 73 61 76 00`) and 2
at `0x0071`.

### 8.4 `city.txt`

With the maps of 8.3 and the world of 8.5 (350 by 300 pixels):

```text
[Area 00]
area_name=Test Village
world_pos=175,125
start_state=On
size=Large
townmap_art_idx=156
entrance_0=On,100,200,Test Village,-1,-1,0
entrance_2=Off,150,220,Temple,1,12345,-1
entrance_3=On,10,10,No Such Map,-1,-1,0
[Area 01]
world_pos=99999,5
```

Expected: A is 1, because area 1 is dropped (its X is outside the world).
Area 0 is known, unlocked and Large, with picture index 156 and label -1.
Its entrance count E is 3: slot 0 is known and goes to map 1 at the
header's default entrance; slot 1 is absent; slot 2 is unknown and goes to
map 2 at elevation 1, hex 12345, with the header's orientation. Slot 3 is
dropped (unknown map), so it does not count toward E. Map areas: map 1 is in
area 0 and map 2 in area 0; map 0 has area -1. The start area of section 6.5
is area 0, and its current entrance is slot 2, the lowest slot naming
`artemple`.

### 8.5 `worldmap.txt`

```text
[Data]
terrain_types=Desert:1, Mountain:2
Common=22%
Rare=4%
[Tile Data]
num_horizontal_tiles=1
[Tile 0]
art_idx=339
0_0=Desert,No_Fill,Common,Rare,None,Test_Desert
1_0=Mountain,Fill_W,Rare,Rare,Rare,Test_Desert
6_5=Swamp,Fill_Q,Bogus,Common,Common,Missing_Table
[Random Maps: Desert]
map_00=Desert Encounter 1
[Encounter Table 0]
lookup_name=Test_Desert
maps=Desert Encounter 1
enc_00=Chance:25%, Enc:(2-4) TEST_Geckos
enc_01=Chance:15%, Counter:1, Enc:(1) TEST_Geckos FIGHTING (1-2) TEST_Hunters
enc_02=Chance:10%, If(Global(3) > 1), Enc:(1) TEST_Hunters AMBUSH Player
enc_03=Chance:5%, Special, Counter:1, Map:Temple
enc_04=Chance:5%, Enc:(3-1) TEST_Geckos
[Encounter: TEST_Geckos]
position=surrounding, spacing:2, distance:6
type_00=Ratio:75%, pid:16777217
type_01=Ratio:25%, pid:16777218, Item:(1-3)41, Item:4(wielded)
[Encounter: TEST_Hunters]
type_00=pid:16777219, Script:12, If(Player(Level) >= 5)
type_01=pid:16777220, Dead
```

Parse:
- Frequencies: Forced 100, Frequent 38, Common 22, Uncommon 12, Rare 4,
  None 0. Terrains: 0 Desert (1), 1 Mountain (2).
- `H` = 1 and `T` = 1, so C = 7, R = 6 and the world is 350 by 300.
- Cell (0, 0): Desert, `No_Fill`, morning 22, afternoon 4, night 0, table 0.
- Cell (1, 0): Mountain, `Fill_W`, 4/4/4, table 0. Pixel (75, 10) lies in it.
- Cell (6, 5): terrain 0 (Swamp unknown), `No_Fill` (Fill_Q unknown),
  morning 0 (Bogus unknown), afternoon and night 22, no table
  (Missing_Table unknown).
- Cell (2, 3), with no key: Desert, `No_Fill`, 0/0/0, no table.
- Table 0: enc_00 has one group (2 to 4, TEST_Geckos). enc_01 has counter 1
  and groups (1, TEST_Geckos) FIGHTING (1 to 2, TEST_Hunters). enc_02 has a
  condition and (1, TEST_Hunters) AMBUSH Player. enc_03 is Special, with
  counter 1 and map 2, and no groups. enc_04 is dropped (3 > 1).
- TEST_Geckos: surrounding, spacing 2, distance 6. Type 1 has items 1 to 3
  of PID 41, and PID 4 wielded.

Selection, in the morning (tick 264600, 07:21) at cell (0, 0), player Luck 5,
no Explorer, global 3 = 0, level 1:

1. `random(1, 100)` gives 22: an encounter, since 22 <= 22 (23 would give
   none).
2. Eligible: enc_00, enc_01 and enc_03; enc_02 fails its condition.
3. enc_03 is Special, so three `random(1, 6)` are drawn: 3, 3, 3. Then
   9 - 5 + 5 + 0 = 9, below 18, so enc_03 becomes ineligible.
4. S = 25 + 15 = 40. `random(1, 40)` gives 30, which picks enc_01 (draws 1-25
   would pick enc_00). Its counter goes from 1 to 0.
5. Map: no `Map:`, and the table's `maps` has one item, so no draw: map 0.
   Map 0 has one start point, so no draw: elevation 0, hex 19086.
6. Group TEST_Geckos: count 1 (no draw), direction draw, two eligible types,
   type draw `random(1, 100)`: 1-75 gives PID 16777217.
7. Group TEST_Hunters: `random(1, 2)`, direction draw. Type 0 fails
   `Player(Level) >= 5`, so type 1 is the only eligible type, with no type
   draw: dead critters of PID 16777220.

Repeated with the same state, enc_01's counter is 0, so S = 25 and the draw
is `random(1, 25)`.

Condition evaluation: `If(Global(3) > 1 And Rand(50%))` with global 3 = 0
still draws `random(1, 100)`, since there is no short-circuit, and is false.
With global 3 = 2 and a draw of 51, it is false; with a draw of 50, true.

Travel on difficulty 2 for six iterations: steps on iterations 1, 3 and 5;
`Q` is charged six times. Fill: when the reveal sets cell (1, 0) to 2, its
`Fill_W` sets cell (0, 0) to 2.

### 8.6 `party.txt`

```text
[Party Member 0]
party_member_pid=16777300
area_attack_mode=always, be_sure, nonsense
attack_who=closest
level_minimum=2
level_up_every=3
level_pids=16777301,16777302
[Party Member 1]
attack_who=strongest
```

Expected: record 0, PID 16777300, `area_attack_mode` choices [0, 2]
(`nonsense` dropped), `attack_who` [4], the other five lists empty, levels
from 2 every 3, level PIDs [16777301, 16777302]. Objects of PIDs 16777300,
16777301 and 16777302 are candidates of record 0. Record 1 is dropped (no
PID), so `P_T` is 1.

### 8.7 `quests.txt` and `holodisk.txt`

```text
# location, description, gvar, display, completed
1500, 100, 7, 1, 3
1500, 101, 8, 2, 2
1501, 102, 9, 0, 5   ; trailing comment
1502, 103
```

With globals 7 = 2, 8 = 1 and 9 = 5: quest 0 is active, quest 1 hidden
(1 < 2) and quest 2 completed. The fourth line is dropped. Groups: 1500
with quests 0 and 1, listed because quest 0 is visible; 1501 with quest 2.

```text
12, 400, 1000
13, 401, 2000
```

With global 12 = 1 and 13 = 0, the disk list holds ordinal 0 only, titled by
`pipboy.msg` 400, with content from 1000 to at most 1499. A value of -1 in
global 12 would not acquire it.

### 8.8 `SAVE.DAT` header

Start from SPEC-008 §4.3's 30051-byte header and replace these ranges:

| Offset | Replacement bytes |
| --- | --- |
| `0x0018` | `00 01 00 02 52` |
| `0x0065` | `00 07 00 19 08 C1 00 04 09 98 00 00 00 02` |
| `0x0073` | `41 52 54 45 4D 50 4C 45 2E 73 61 76 00 00 00 00` |
| `0x74E3` | `56 41 55 4C 54 45 4E 47 20 46 32 53 41 56 45 31` |
| `0x74F3` | `00 00 02 BC` |

Expected: version 1.2R, game date 25 July 2241 (month 7, day 25, year
2241), tick 264600 (07:21), elevation 0, map 2, file `ARTEMPLE.sav`, tag
present, G = 700, gated mark 0. With an installed G of 700 the header
passes, whether or not scripts are gated. Each of these mutations makes the
slot fail before any working file changes:

- version minor 1, which is a Fallout 1 slot;
- any tag byte changed;
- G written as 701;
- byte `0x74F7` set to 2;
- byte `0x74F7` set to 1 once the script gate is open (while it is still
  gated, the slot passes);
- the header cut short.

Calendar: tick 264600 + 7 x 864000 = 6312600 is 1 August 2241, 07:21. Tick
160 x 864000 = 138240000 is 1 January 2242, 00:00.

### 8.9 World-map component

The installed tables are those of 8.3 to 8.5: C = 7, R = 6, one area with E
= 3, `M_T` = 3, and two limited entries (enc_01 and enc_03 of table 0). The
state:
- position (175, 125), area 0, entrance 0;
- no car: `has_car` 0, `car_area` -1, `car_map` -1, `car_fuel` 0;
- discovery 2 at (0, 0), and 1 at (1, 0), (0, 1) and (1, 1);
- area 0 visited and unlocked, at its `world_pos` (175, 125), with entrance
  states 1, 0, 0;
- map 2 known and visited, maps 0 and 1 neither;
- enc_01 remaining 0 and enc_03 remaining 1.

This is the complete 135-byte component:

```text
0000: 00 00 00 83 00 00 00 AF 00 00 00 7D 00 00 00 00
0010: 00 00 00 00 00 00 00 00 FF FF FF FF FF FF FF FF
0020: 00 00 00 00 00 07 00 06 02 01 00 00 00 00 00 01
0030: 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
0040: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
0050: 00 00 00 00 00 01 02 00 00 00 00 AF 00 00 00 7D
0060: 03 01 00 00 00 00 00 03 00 00 03 00 00 00 02 00
0070: 00 00 00 00 00 00 01 00 00 00 00 00 00 00 00 00
0080: 00 00 03 00 00 00 01
```

L = 131 (`0x83`). The discovery bytes run from `0x28` to `0x51`. A starts at
`0x52`; the area record `02 00`, X, Y, `03 01 00 00` at `0x56`; M = 3 at
`0x64` and the flags `00 00 03` at `0x68`; K = 2 at `0x6B`; the records (0,
1, 0) and (0, 3, 1) follow. Each of these refuses the load: L changed to
132; R written as 5; E written as 2; M written as 4; a known byte of 3;
`has_car` 2; `car_fuel` 80001.

### 8.10 Party record

A member of record 1, at level stage 2, with SPEC-019 §3.15.1's `defensive`
preset (disposition 2, `be_sure` 2, `bleeding` 3, `ranged_over_melee` 3,
`stay_close` 0, `whomever_attacking_me` 0, `stims_when_hurt_little` 1),
writes these 36 bytes after the identifiers:

```text
00 00 00 01 00 00 00 02 00 00 00 02 00 00 00 02 00 00 00 03
00 00 00 03 00 00 00 00 00 00 00 00 00 00 00 01
```

With P = 2 and no retained records, the next 4 bytes are Q = `00 00 00 00`.
Read back under 8.6's tables, where only record 0 exists, it refuses the
load. Under tables with records 0 and 1, where record 1 has two level PIDs,
it loads. A `best_weapon` word of 8 is then read as `no_pref`, the neutral
value. P = 33 refuses the load.

### 8.11 Automap database

The empty database is 12005 bytes: version `01`, length `00 00 2E E5`, then
3000 words. With 8.3's maps: map 0 (excluded) has -1 at all three
elevations; maps 1 and 2 have 0; maps 3 to 999 (absent) have -1. The word for
map 2, elevation 2 is at offset 37 (`0x25`).

### 8.12 Elevators

With 8.3's maps, `data/vault-engine/elevators.txt`:

```text
[Elevator 0]
background=el_test
stop_0=2, 0, 14104, 1
stop_1=2, 1, 22504, 2
stop_3=1, 0, 100, G
doors=33554585
[Elevator 1]
background=el_test
stop_0=2, 0, 100, 1
stop_1=7, 0, 100, 2
```

Type 0 has three buttons: stops 0, 1 and 3, renumbered 0, 1 and 2, with keys
`1`, `2` and `G` (and `g`). Its door list is scenery PID `0x02000099`. Type 1
is dropped: map 7 has no record, so it keeps one stop. With the player on
map 2, elevation 1, the start level of type 0 is button 1. `metarule(15, 1)`
and `metarule(15, 5)` do nothing, with one diagnostic each. With no file at
all, every type does nothing.

### 8.13 Play while scripts are gated

Build a synthetic install (section 2.2's members, holding the tables of 8.3
to 8.6) whose `maps/artemple.map` header names a map script, entrance hex
20100, elevation 0, orientation 2, and one critter whose record names
script 4. Start Play with the gate closed:

- the new game reaches the main loop with the player at hex 20100,
  elevation 0, facing 2, on map 2, at world position (175, 125);
- the VFS read log holds no `.int` path, and the VM run list is empty;
- the message monitor's last line is SPEC-017 §8.7's notice, exactly once;
- the critter's script record is present, with its stored local variables;
- a save made now has byte `0x74F7` = 1 and contains that record in
  `ARTEMPLE.SAV`;
- walking to an exit grid loads its destination map, and the notice is not
  repeated for that map change.

### 8.14 Line tables

`genrep.txt`:

```text
-50, 1001
0, 1002
0, 1009
100, 1003
```

Karma -60 gives no title; -50 to -1 give 1001; 0 to 99 give 1002 (the first
of the two threshold-0 records); 100 and more give 1003.

`endgame.txt`:

```text
5, 1, 327, nar_a1
5, 2, endslid2, nar_a2, -1
9, 0, 330, nar_b, 2
```

Records: (global 5 = 1, art row 327, `nar_a1`, still); (global 5 = 2, art
`art/intrface/endslid2.frm` with `endslid2.pal`, `nar_a2`, pan right to
left). The third line is dropped (direction 2).

## 9. Coordination

**SPEC-016** (maps and prototypes). This specification relies on SPEC-016 for
the version-20 map stream (laid out as version 19, per the coordinator's
brief) and the 416-byte critter prototype. Saved maps (section 7.7) and the
player object (section 7.3, order 5) follow it.

The three Fallout 2 specifications were reconciled on 2026-10-25 (decisions
R1-R6). Each point below holds in SPEC-017, SPEC-018 and SPEC-019 alike.

**SPEC-017** (host words, VM deltas, the script gate):
- `load_map` and every word taking a map name or number use section 5.1. The
  entrance parameter is stored in no campaign global (section 5.1, R1).
- `mark_area_known` type 0 acts on section 5.8's area known state, type 1
  on section 5.2's map flags; `metarule` 17 and 19 read them. `metarule3`
  104 uses section 5.8's "entrances by map". `wm_area_set_pos` writes
  section 5.8's area position. All are saved in section 7.4.
- `metarule` 30-32 and `metarule3` 110 act on SPEC-019's four car values,
  saved in section 7.4. No word sets or reads a carry amount, and no word
  returns a visible party count: SPEC-019 has neither (R2).
- `metarule` 15 opens the elevators of section 4.9 (R3).
- `play_gmovie` indexes the 17-entry table of section 6.4 and SPEC-019
  §3.21. Kill categories are 0-18 (section 7.3).
- `metarule3` 109 reads the control record's `chem_use` (section 4.7 leaves
  the packets' Fallout 2 keys unread).
- Script execution stays gated by SPEC-017 §8.5. SPEC-017 §8.7 defines Play
  while gated, which section 6.7 and SPEC-019 §3.4 follow (R4).

**SPEC-019** (interface, party control, car) owns the behavior for this data:
- party control: the `party.txt` allowed sets and level PIDs (section 4.5),
  with the party list, control records, level stages and retained records
  saved in section 7.5;
- the car's four values, saved in section 7.4, including the fuel range and
  car travel;
- the world-map and town-map screens: area `size` and containment, art
  indices, entrance marker positions, the viewport, the encounter flash,
  dropping to terrain (using section 5.7);
- `pipboy_active` (SPEC-019 §3.10), the Pip-Boy pages of section 5.9, the
  Karma folder from section 4.6's tables, the endgame slides from
  `endgame.txt`;
- the premade screen (section 6.3), movie flags and callers (section 6.4);
- the save/load screen's location names (section 5.2) and the "old version"
  state (section 7.2);
- the names of kill types 0-18, and trait 13's name.

Parsed but without effect in this revision: `ambient_sfx`, `encounter_difficulty`,
the formation names, and the relations `AND`, `FIGHTING` and `AMBUSH Player`
(section 5.7).

## 10. Decisions

"Owner instruction" means the project owner or the coordinator's brief
decided it. "Team A default" means Team A chose it with no source, as
AGENTS.md requires for open points.

| # | Decision | Kind |
| ---: | --- | --- |
| D1 | Archive roles and `patch000.dat` above `MASTER.DAT` (section 1) | Owner instruction, 2026-10-25 |
| D2 | SPEC-016 maps are version 20 with the version 19 layout; critter prototypes are 416 bytes | Owner instruction (coordinator's brief) |
| D3 | Slots belong to one game; a Fallout 1 slot never loads under Fallout 2 | Owner instruction (roadmap) |
| D4 | Archive discovery: the install folder only, case-folded names, regular files, ambiguity refused, an unreadable `patch000.dat` refuses the install, no other patch names | Team A default |
| D5 | `fallout2.cfg` archive-path keys are not read | Team A default |
| D6 | The seven required members of section 2.2; no required member in `CRITTER.DAT`; quests and holodisks optional | Team A default |
| D7 | The table text grammar, value forms and the three severities (sections 3.3-3.5), including `;` comments, the later repeated key winning and merged sections | Team A default |
| D8 | `maps.txt` keys, defaults, numbers 0-999, 8-byte stems, and name matching by stem plus `.MAP` | Team A default |
| D9 | `city.txt` keys, defaults, the seven entrance items and their -1 meanings, slots 0-31, areas 0-999 | Team A default |
| D10 | `worldmap.txt` grammar; the default frequency names and percentages; the default terrain list; 350 by 300 tiles of 7 by 6 subtiles; at most 16 tiles across and 256 tiles | Team A default |
| D11 | The entry, group-expression and condition grammars; left-to-right evaluation without short-circuit; unparsable conditions false | Team A default |
| D12 | Walk masks are `data/<stem>.msk`, 13200 bytes, most significant bit first | Team A default |
| D13 | `party.txt` keys and the seven option vocabularies with their indices | Team A default |
| D14 | `quests.txt` and `holodisk.txt` grammars, their message files, the quest status rule and grouping | Team A default |
| D15 | Which files are left to later specs; how the compiled tables are replaced; overrides not read for world tables; Fallout 1 critical tables kept provisionally, with kill types 15-18 on row 0 | Team A default |
| D16 | A map's area is the lowest area with an entrance naming it; the first-entry and locking rules | Team A default |
| D17 | The terrain-difficulty schedule; `W`, `Q` and healing unchanged; the walk-mask test. (Script-directed travel: see R6) | Team A default |
| D18 | Fill reveals to the grid edge in the named directions, setting 2 | Team A default |
| D19 | The encounter check: one `random(1, 100)` against the period's percentage at each SPEC-008 check; the three periods. (Suppression: see R6) | Team A default |
| D20 | Entry selection, the reuse of SPEC-008's special test as the Special gate, and the counters | Team A default |
| D21 | Encounter entrance choice, group spawning and placement; relation keywords have no engine effect in this revision | Team A default |
| D22 | Rest permission by `can_rest_here`, the last-attacker test, and the team test only outside areas; aging without the Fallout 1 special case, with the remains prototype kept provisionally | Team A default |
| D23 | New game on `maps/artemple.map` at its header entrance; the start-area rule; the initial world state; the order of the hand-off | Team A default |
| D24 | Reset tick 264600; calendar day 0 is 25 July 2241 | Team A default |
| D25 | The premade files and order; the 428-byte sheet unchanged; trait 13 named Sex Appeal | Team A default |
| D26 | The 17-entry movie table; startup `iplogo` and `intro`; `elder.mve` at a new game | Team A default |
| D27 | No campaign global has a fixed engine meaning here; the Fallout 1 water, world-map and Pip-Boy globals are switched off | Team A default |
| D28 | Save version 1.2, the layout tag and G in the reserved bytes, the read checks, and no claim of interoperability with original slots | Team A default |
| D29 | Payload counts: 19 kill counters, 119 perk ranks for the player only, 17 movie bytes, queue types 0-12 only, and the snapshot's final byte kept and written 0 | Team A default |
| D30 | The world-map component layout and its read checks (revised by R2, R5, R6) | Team A default |
| D31 | The party component's 36-byte member records (revised by R5) | Team A default |
| D32 | The automap index of 1000 map numbers and the excluded set from `saved=No` | Team A default |
| D33 | Saves stay big-endian under Fallout 2 | Team A default |

**Reconciliation decisions** (Team A default, reconciliation 2026-10-25). They
resolve conflicts between SPEC-017, SPEC-018 and SPEC-019, and each is
written the same way into every spec it touches.

| # | Conflict | Rule chosen | Kind |
| ---: | --- | --- | --- |
| R1 | `load_map`'s entrance global: SPEC-017 kept element 32, while SPEC-018 and SPEC-019 switched Fallout 1's indexes off | No campaign global receives the entrance parameter, or any other value from the engine, under Fallout 2 (sections 5.1, 6.2). A wrong index would silently corrupt an unrelated Fallout 2 campaign variable | Team A default, reconciliation 2026-10-25 |
| R2 | A visible party count and a car carry amount expected by SPEC-019, with no SPEC-017 word for them | No word is invented. SPEC-019 keeps the visible count for its own internal rules only and drops `car_carry`, so the car has four saved values (section 7.4) | Team A default, reconciliation 2026-10-25 |
| R3 | No Fallout 2 elevator table | An engine-owned optional table `data/vault-engine/elevators.txt` (section 4.9), with SPEC-013 §3.21's behavior and all four of its safe rules. Without a record, `metarule` 15 does nothing | Team A default, reconciliation 2026-10-25 |
| R4 | SPEC-017's script gate against the owner's goal that Play unlocks | Play is available, and only script execution is gated: no `.INT` loads and no procedure runs, script records are kept, and a one-line notice appears (SPEC-017 §8.7, section 6.7). Saves made while gated carry a header mark (section 7.2) | Team A default, reconciliation 2026-10-25 |
| R5 | Terms and stored fields: area against city; disposition indices; the order of the control fields; the saved party state | Area and city are one record (section 4.3). Disposition indices are SPEC-019's five tokens, with no `none`. Member records use SPEC-019 §2.5's field order, store the level stage, and are joined by retained records for members who left (section 7.5). The 17-entry movie table and `elder.mve` agree in all three specs | Team A default, reconciliation 2026-10-25 |
| R7 | The retail encounter conditions are `If(...) And If(...)` (1004 of 1033) | The condition field of section 4.4.6, with spaces allowed after `If` and case folding | Coordinator, Revision 2, measured |
| R8 | 375 special entries name their group with no count; 3 entries end `FIGHTING Player` | A group without a count is `(1)`; `FIGHTING Player` is accepted, and relations still have no effect | Coordinator, Revision 2, measured |
| R9 | 10 encounter types place item or scenery PIDs | Types may name categories 0, 1 or 2, and non-critter types place the object | Coordinator, Revision 2, measured |
| R10 | `Distance:` in 13 types and in one `position`, written as `Player(Perception)`; one `team_num` key | Per-type distance; `Player(Perception)`; `team_num` stored with no effect | Coordinator, Revision 2, measured |
| R11 | 5 items use `{Wielded}` | Accepted as `(wielded)`. The 2 retail item fields that run two items together with no comma stay dropped, as before | Coordinator, Revision 2, measured |
| R12 | Area 47 has two entrances at X and Y -1 | -1, -1 is an entrance with no marker | Coordinator, Revision 2, measured |
| R13 | Party record 25's PID is a non-critter (the car trunk); 14 records write `level_pids=-1` | Non-critter records are accepted as followers; `-1` is the empty list | Coordinator, Revision 2, measured |
| R6 | Other contradictions | Script-directed travel is SPEC-019's direct move. Encounter suppression uses SPEC-019's area circles. This spec defines dropping to terrain, the map known/visited flags, the saved area positions, the `metarule3` 104 entrance match, the world-map music, display and location names, and the `genrep.txt`, `karmavar.txt` and `endgame.txt` grammars. SPEC-011's progression tables apply provisionally. The Fallout 2 `ai.txt` keys and town reputations are not read | Team A default, reconciliation 2026-10-25 |

## 11. Verification notes

- [x] No source listing, struct definition or internal symbol name of any
  reference executable.
- [x] No path under a source-code directory and no 40-character hex string
  (the CLAUDE.md ingestion patterns were checked by search).
- [x] Every multi-byte field's endianness is stated (section 3.2).
- [x] All test vectors are synthetic. The hex of 8.8 to 8.11 was computed
  independently from the layout tables, and recomputed after the
  reconciliation pass (R1-R6).
- [x] Every rule is tagged `[Must-match]`, `[Engine layout]`, `[Safe rule]`
  or `[May-improve]`, or inherits its tag from the cited section.
- [ ] Not checked against the research vault's `spec_lint.py`, which is not
  available on this side of the wall.
- [x] Checked against the owner's retail Fallout 2 install (Revision 2,
  2026-10-25). The discrepancies found are corrected above (R7 to R13).
  The diagnostics that remain on the retail files are genuine data
  defects, all of which section 4's safe rules drop:
  - one start point at elevation 3;
  - one `music` line with no `=`;
  - one `ambient_sfx` list missing a comma;
  - two item fields running two items together;
  - two repeated keys, of which the later is kept.
