# SPEC-016: Map Version 20 and Fallout 2 Prototypes

**Status:** approved by the project owner, 2026-10-25. Revision 2 (retail
corpus audit), 2026-10-25.

**Provenance.** Written by the coordinator from the project owner's layout
brief of 2026-10-25, against the approved SPEC-003. No game executable,
decompilation or re-implementation source was consulted. The points the brief
leaves open are resolved in section 5 (Decisions), each marked as an owner
instruction or a coordinator default. See `PROVENANCE.md`, "In-session spec
authoring".

**Revision 2 (retail corpus audit), 2026-10-25.** The coordinator wrote this
revision at the owner's request, after running the engine's own SPEC-016
parsers over the owner's retail Fallout 2 install (GOG release: `MASTER.DAT`,
`CRITTER.DAT`, `patch000.dat`). sfall's archive, `f2_res.dat` and the `mods`
folder were neither mounted nor opened. The revision cites counts, file names,
object PIDs and record lengths only; no game bytes are reproduced. It
changes section 2's ladder records and section 3.1's record length, and adds
Decisions 8 and 9 and two vectors.

## 1. Scope

This specification adds Fallout 2's map and prototype records to SPEC-003. The
game profile (`game = "fallout1"` or `"fallout2"`) selects the layout, together
with the exact version and length checks below:

- Under `fallout1` every SPEC-003 rule is unchanged. `[Must-match]`
- Under `fallout2`, the rules below replace SPEC-003's version and critter
  rules. Everything else is as SPEC-003. `[Must-match]`

## 2. Maps

- A Fallout 2 map has **version 20** (`0x00000014`) in its first word, where
  SPEC-003 §2.7 has 19. `[Must-match: owner instruction]`
- Every other part of the map is exactly as SPEC-003's version 19 map
  `[Must-match: owner instruction]`:
  - the same 236-byte (`0xEC`) fixed header, field for field;
  - the map-variable (MVAR) and local-variable (LVAR) pools;
  - the three elevations' tile grids, with the same omission flags;
  - the same five script categories and their blocks;
  - the same object records: the 88-byte base record (22 big-endian 32-bit
    words) and its per-category sub-records, with inventories, except the
    ladder records below.
- **Ladders (Revision 2).** A scenery object whose prototype subtype is
  ladder up or ladder down (SPEC-003 §2.11) carries **two** additional words
  under `fallout2`, where SPEC-003 has one:
  1. the destination map number, a `maps.txt` record number (SPEC-018 §4.2);
  2. the packed destination, exactly as SPEC-003's single ladder word.

  The record is 96 bytes before its children, like stairs. The ladder
  prototype itself is unchanged: 45 bytes, one destination word. Under
  `fallout1` a ladder record keeps its one word. `[Must-match: retail
  corpus, Decision 8]`
- Under `fallout2` a map must have version 20; any other version, 19
  included, is refused. Under `fallout1` a map must have version 19 (SPEC-003),
  so version 20 is refused there. `[Safe rule]`
- A map's version is kept with it. Writing a map back (saves' working maps,
  SPEC-008) writes the version it was read with, so a Fallout 2 map stays
  version 20. A new empty map is version 19 under `fallout1` and version 20
  under `fallout2`. `[Safe rule]`

## 3. Prototypes

### 3.1 Critters

A Fallout 1 critter prototype (SPEC-003 §2.5) consumes 412 bytes (`0x19C`). A
Fallout 2 critter prototype is **416 bytes** (`0x1A0`): the same 412 bytes,
then one more big-endian 32-bit word at offset `0x19C`. `[Must-match: owner
instruction]`

| Offset | Size | Field |
|---|---|---|
| `0x000`-`0x19B` | 412 | as SPEC-003 §2.5 |
| `0x19C` | 4 | `damage_type`: u32, the critter's unarmed (melee) damage type |

- Under `fallout2`, a critter prototype file must be exactly 416 bytes, or
  exactly 412 bytes (Revision 2). A 412-byte record is read with SPEC-003
  §2.5's layout, takes damage type 0 (normal), and gives one diagnostic. Any
  other length is refused. `[Safe rule]` (Decision 9)
- Under `fallout1`, critter prototypes are read as SPEC-003 says, and their
  damage type is **0** (normal). `[Must-match: owner instruction]`
- `damage_type` takes the damage-type numbering of SPEC-011's weapon
  `damage_type` field (0 is normal). A value outside that numbering is kept as
  read, and the engine's damage rules treat it as normal (0) with a
  diagnostic. `[Safe rule]`
- Writing a Fallout 2 critter prototype writes the length it was read with:
  416 bytes, or 412 for a record read at 412 (Revision 2). A new Fallout 2
  critter prototype is 416 bytes. Writing a Fallout 1 one writes 412, as
  before. `[Safe rule]`

### 3.2 Other categories

Item, scenery, wall, tile and misc prototypes keep the SPEC-003 layout
unchanged under both profiles. `[Must-match: owner instruction]`

## 4. Where the profile reaches

- The engine reads every map and prototype through the active game profile:
  - maps the world loads;
  - the working maps of saves (SPEC-008);
  - the prototypes the asset catalog reads.

  A file in the other game's layout is refused with a message naming the
  version or length. `[Safe rule]`
- Map preview (`--preview-map`) under `--game-profile fallout2` loads a map
  from the SPEC-018 §1 databases (SPEC-015 archives) and renders it like a
  Fallout 1 map. `[Safe rule]`

## 5. Decisions

1. **Header, pools, grids, scripts and objects** are unchanged in version 20
   (owner instruction).
2. **The critter record** gains one trailing word, `damage_type`, at `0x19C`
   (owner instruction).
3. **Other prototype categories** are unchanged (owner instruction).
4. **The format is chosen by the profile together with the exact
   version/length**, and neither alone decides (owner instruction). So a
   version 19 map is refused under `fallout2` and a version 20 map under
   `fallout1`, rather than read by its version alone (coordinator default:
   one layout per profile keeps each game's rules exact).
5. **A critter file's length is checked exactly (416 bytes) under
   `fallout2`.** Fallout 1's reader keeps SPEC-003's own length rules
   (coordinator default, following the owner's "exact record length checks").
6. **`damage_type` uses SPEC-011's damage-type numbering**, and an unknown
   value behaves as normal damage (coordinator default).
7. **A new empty map takes the profile's version** (coordinator default).
8. **Fallout 2 ladder records carry a destination map before the packed
   destination** (Revision 2, measured). With SPEC-003's one-word ladder
   record, 24 of the 155 retail maps fail part-way through their object
   list, because each failure follows a ladder object. With two words, all
   155 maps parse. All 62 ladder records then hold a first word inside
   `maps.txt`'s record range and a packed destination with a hex below
   40000 and an elevation of 0-2.
9. **Two retail critter prototypes are 412 bytes** (Revision 2, measured):
   `proto/critters/00000077.pro` and `00000114.pro`, out of the install's
   critter prototypes. Refusing them would refuse retail data, so they are
   read with Fallout 1's layout and damage type 0 (coordinator default).
   They keep their length when written back.

## 6. Test vectors

The tests build every vector from synthetic data:

- the synthetic version 19 map fixture with its version word set to 20,
  parsed under both profiles: accepted under `fallout2`, refused under
  `fallout1`; and the unchanged fixture refused under `fallout2`;
- a version 20 map written back, which keeps version 20 and the same bytes;
- a synthetic critter prototype extended to 416 bytes with `damage_type = 3`:
  parsed under `fallout2` with `damage_type` 3, written back to the same 416
  bytes, and refused under `fallout2` at 412 or 420 bytes;
- the 412-byte record under `fallout1`, with damage type 0;
- (Revision 2) the 412-byte record under `fallout2`: accepted with damage
  type 0 and one diagnostic, and written back as the same 412 bytes;
- (Revision 2) a version 20 map holding a ladder-up object whose prototype is
  the synthetic 45-byte ladder: under `fallout2` its record is 96 bytes, with
  destination map 7 and packed destination `0x00004E84` (hex 20100, elevation 0), followed by a second
  object that parses. Under `fallout1`, a version 19 map's ladder record is
  still 92 bytes (SPEC-003). Written back under `fallout2`, the map gives the
  same bytes;
- an end-to-end map preview: a synthetic Fallout 2 install with
  `MASTER.DAT` and `CRITTER.DAT` as SPEC-015 archives, holding the synthetic
  corpus with its map at version 20 and its critter prototypes at 416 bytes,
  loaded and rendered under `--game-profile fallout2 --preview-map`.
