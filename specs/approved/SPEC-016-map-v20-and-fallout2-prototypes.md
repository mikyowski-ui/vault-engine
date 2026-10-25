# SPEC-016: Map Version 20 and Fallout 2 Prototypes

**Status:** approved by the project owner, 2026-10-25.

**Provenance.** Written by the coordinator from the project owner's layout
brief of 2026-10-25, against the approved SPEC-003. No game executable,
decompilation or re-implementation source was consulted. The points the brief
leaves open are resolved in section 5 (Decisions), each marked as an owner
instruction or a coordinator default. See `PROVENANCE.md`, "In-session spec
authoring".

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
    words) and its per-category sub-records, with inventories.
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

- Under `fallout2`, a critter prototype file must be exactly 416 bytes; any
  other length is refused. `[Safe rule]`
- Under `fallout1`, critter prototypes are read as SPEC-003 says, and their
  damage type is **0** (normal). `[Must-match: owner instruction]`
- `damage_type` takes the damage-type numbering of SPEC-011's weapon
  `damage_type` field (0 is normal). A value outside that numbering is kept as
  read, and the engine's damage rules treat it as normal (0) with a
  diagnostic. `[Safe rule]`
- Writing a Fallout 2 critter prototype writes all 416 bytes; writing a
  Fallout 1 one writes 412, as before. `[Safe rule]`

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
- an end-to-end map preview: a synthetic Fallout 2 install with
  `MASTER.DAT` and `CRITTER.DAT` as SPEC-015 archives, holding the synthetic
  corpus with its map at version 20 and its critter prototypes at 416 bytes,
  loaded and rendered under `--game-profile fallout2 --preview-map`.
