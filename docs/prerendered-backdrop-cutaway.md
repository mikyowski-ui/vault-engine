# Owner brief: pre-rendered map backdrops with depth and foreground fade in `vault-render`

Basis: `vault-engine` at revision `4d5183a`. Modules: `vault_render::map`
(the CPU reference compositor: `tiles`, `objects`, `roof`, `lighting`,
`render_floor_lit`), `vault_render::map::enhanced` (roof peek),
`vault_render::layers` (`FrameLayer`, `SortKey`, the instanced CPU executor,
in its instanced-layers module) and
`vault_render::layers::gpu` (the wgpu pass, which creates its pipelines with
`depth_stencil: None`); `vault_formats::map` (`MapFile`),
`vault_formats::geometry` (`HexGrid`), `vault_formats::dat` (LZSS block
decoding), `vault_formats::inflate` (zlib), `vault_formats::palette`;
`vault_core::world` (blocking, roof state, light). Specifications: SPEC-001
§3.7 "Generated intensity and mix tables"; SPEC-003 §2.8 "Variable pools and
roof/floor tile blocks", §2.10 "Per-elevation object streams and fixed
record prefix", §3.6 "Hex and floor/roof world-to-screen projection", §3.7
"Screen-to-grid projection and integer boundary rules"; SPEC-007 §3.1 "Hex
Route Search, Blockers, Doors, and Replanning" and §3.2 "Perception, LOS,
Shootability, and Ray Traversal"; SPEC-014 §3.36 "Roof Hiding and
Restoring", §3.37 "Player See-Through Cutout", §3.44 "Map Object Draw Order,
Lighting and Translucency"; SPEC-021 §3.1 "What the Hash Covers, and Where Each Value Comes From"; SPEC-022
§2.1 "Frame Layers and the Sort Key", §3.8 "The Hex Light Buffer", §3.9
"Floor Light", §3.10 "The Roof Peek Mask", §3.11 "GPU Requirements", §3.12
"Fallback and Errors", §3.13 "What the Path Never Changes".

Tags: `[Must-match]` must be exactly as stated, and leaves every map without
a backdrop drawn byte for byte as today; `[May-improve]` an implementation
freedom that changes no canvas byte; `[Safe rule]` what the engine does with
data it cannot honour.

## 1. Purpose

Mods with pre-rendered rooms want to ship each map elevation as one large
painted image instead of thousands of floor tiles and wall sprites, and
still have critters, items and doors pass in front of and behind the
painted walls correctly, with the player never hidden behind a front wall.
Today the engine has no way to do this.

What the engine does today (`4d5183a`):

1. **No depth.** Neither path has a depth buffer: the GPU pipelines use
   `depth_stencil: None`, and SPEC-022 §3.11 item 4 allows a depth test
   only if every canvas byte stays the same.
2. **Painter's order.** A refresh draws layers 0-7: Floor, Flat, Upright,
   Roof, Outline, FloatingText, NoHex, Cursor (SPEC-022 §2.1). The layer is
   the top three bits of the 64-bit sort key, and all eight values are in
   use. Upright objects draw by ascending hex number and, within a hex, by
   list order; no screen-y or sprite-bottom sort is made (SPEC-014 §3.44).
3. **No backdrop.** Floor art comes only from the 12-bit floor indexes of
   the map's tile words (SPEC-003 §2.8); walls and scenery are objects.
4. **Cutout.** The see-through cutout uses `egg.frm` (`intrface.lst` index
   2, 129 by 98, weights 0-122, shifts (0, 25)) placed on the player. Every
   drawn roof square goes through it; a wall or scenery object goes through
   it only when the player is not hidden, the object has none of the
   translucency bits, and the SPEC-014 §3.37 orientation test passes.
5. **Roofs.** The square under the player and its flood-filled region are
   hidden (SPEC-014 §3.36). The enhanced `cursor_xray` roof peek adds a
   96-pixel disc with a 48-pixel dithered ring around the cursor
   (SPEC-022 §3.10).
6. **Blocking.** Movement and sight blockers are visible objects: a hidden
   object neither blocks movement nor stops sight (SPEC-007 §3.1, §3.2). So
   an "invisible blocker" cannot be a hidden object.
7. **Light.** Objects are lit by `max(ambient, hex light)`; floor tiles by
   the interpolated floor light; indexes 229-255 stay unlit (SPEC-014
   §3.44, SPEC-022 §3.8-§3.9).
8. **Extent.** The grid's drawn extent is `[0, -1188, 7984, 2404]` in
   default-grid pixels (`docs/REFERENCE.md`, auto-fit floor), about 28.7
   million pixels per elevation.

## 2. Proposed API and files

### 2.1 Files

```text
maps/<map>.bkd                    index: header, elevations, chunk table, suppression list
maps/<map>/e<E>/<CX>_<CY>.bkc     one chunk: colour, depth and fade planes
```

`<map>` is the map's file stem, `<E>` 0-2, `<CX>`/`<CY>` decimal chunk
column and row without padding. Absent `.bkd`: no backdrop. `[Safe rule]`

### 2.2 Rust

In a new `vault_formats::backdrop` module (portable, no host access, so the
SPEC-023 §3.17 gate holds):

```rust
pub struct BackdropIndex {
    pub version: u16,
    pub chunk_size: u16,
    pub palette_hash: u64,
    pub elevations: Vec<BackdropElevation>,
}
pub struct BackdropElevation {
    pub elevation: u8,
    pub planes: Planes,            // colour always; depth and fade optional
    pub origin_hex: i32,
    pub origin_px: [i32; 2],       // image pixel of origin_hex's reference corner
    pub size: [u32; 2],
    pub chunks: [u16; 2],          // columns, rows
    pub present: Vec<bool>,        // row-major, columns × rows
    pub suppress: Suppression,
}
pub enum Suppression { None, Walls, Pids(Vec<u32>), WallsAndPids(Vec<u32>) }
pub struct BackdropChunk {
    pub origin: [u32; 2],          // image pixel of the chunk's top-left
    pub size: [u16; 2],
    pub colour: Vec<u8>,           // palette indexes, 0 transparent
    pub depth: Option<Vec<u16>>,
    pub fade: Option<Vec<u8>>,     // 1 bit per pixel, rows padded to bytes
}
impl BackdropIndex { pub fn parse(bytes: &[u8]) -> Result<Self, ParseError>; }
impl BackdropChunk { pub fn parse(bytes: &[u8], index: &BackdropIndex, e: u8) -> Result<Self, ParseError>; }
```

In `vault_render::map`, the reference compositor takes an optional
`&BackdropView` (the decoded chunks under the refresh rectangle); the
instanced path takes the same data as integer textures.

## 3. Layout and behaviour

Layout, as the parser reads it (sections 3.1-3.4): all multi-byte numbers
big-endian, as in SPEC-003. `[Must-match]` for sections 3.1-3.4 once
approved.

### 3.1 `.bkd` header (24 bytes)

| Offset | Bytes | Field |
| --- | ---: | --- |
| `0x00` | 4 | Magic `56 42 4B 44` (`VBKD`) |
| `0x04` | 2 | Version, 1 |
| `0x06` | 1 | Elevation mask, bits 0-2; at least one bit set |
| `0x07` | 1 | Compression of the chunk files: 0 stored, 1 zlib (RFC 1950) |
| `0x08` | 2 | Chunk size S: 64, 128, 256 or 512 |
| `0x0A` | 2 | Reserved, 0 |
| `0x0C` | 4 | Reserved, 0 |
| `0x10` | 8 | The SPEC-021 §2.1 64-bit hash of the palette file the colour indexes were made for (`color.pal` as the map loads it) |

### 3.2 Elevation record (32 bytes, one per set mask bit, ascending)

| Offset | Bytes | Field |
| --- | ---: | --- |
| `+0x00` | 1 | Elevation, equal to the mask bit |
| `+0x01` | 1 | Planes: bit 0 colour (must be set), bit 1 depth, bit 2 fade |
| `+0x02` | 1 | Suppression: 0 none, 1 wall-category objects, 2 listed PIDs, 3 both |
| `+0x03` | 1 | Reserved, 0 |
| `+0x04` | 4 | Origin hex, 0-39999 |
| `+0x08` | 4 | Origin X: image column of the origin hex's reference corner |
| `+0x0C` | 4 | Origin Y: image row of that corner |
| `+0x10` | 4 | Width W, 1-16384 |
| `+0x14` | 4 | Height H, 1-16384 |
| `+0x18` | 2 | Chunk columns, `ceil(W / S)` |
| `+0x1A` | 2 | Chunk rows, `ceil(H / S)` |
| `+0x1C` | 4 | Suppressed PID count N, 0-4096 |

Then, per elevation in the same order: the presence table, one byte per
chunk (0 absent, 1 present), row-major; then N PIDs of 4 bytes.

**Placement.** Image pixel (x, y) lies at world reference point
`R(origin hex) + (x - origin X, y - origin Y)`, where R is the SPEC-003 §3.6
world reference corner. So it moves with OX/OY like every hex and tile.

### 3.3 `.bkc` chunk file

| Offset | Bytes | Field |
| --- | ---: | --- |
| `0x00` | 4 | Magic `56 42 4B 43` (`VBKC`) |
| `0x04` | 2 | Version, 1 |
| `0x06` | 1 | Elevation |
| `0x07` | 1 | Planes, equal to the elevation record's |
| `0x08` | 2 | Chunk column |
| `0x0A` | 2 | Chunk row |
| `0x0C` | 4 | Payload length after decoding |
| `0x10` | ... | Payload, stored or zlib per the index |

Chunk width `w = min(S, W - S × column)`, height `h` likewise. The decoded
payload is, in order: `w × h` colour bytes; with the depth plane, `w × h`
16-bit depth values; with the fade plane, `h × ceil(w / 8)` bytes, bit 7 of
each byte first. Its length must equal the header's.

### 3.4 Depth encoding

The depth value D of a pixel places it in SPEC-014 §3.44's painter's order,
which is ordered by hex number:

| D | Meaning |
| --- | --- |
| 0 | Floor: drawn at the end of the Floor layer, behind every flat and upright object |
| 1-40000 | Upright at group hex `B = D - 1`: drawn after every upright object on hexes up to and including B, and before every upright object on hexes above B |
| 40001-65535 | Invalid |

So D is "the hex-row-equivalent sort position" exactly as the engine sorts:
a backdrop pixel with D = B + 1 occludes exactly what an opaque upright
object on hex B, drawn last on its hex, would occlude. Without a depth plane
every pixel has D = 0.

### 3.5 Drawing (the reference algorithm)

1. A refresh draws the Floor layer as today, then every backdrop pixel with
   D = 0 and colour not 0, lit (section 3.7). Colour 0 is transparent: the
   floor tile beneath shows. `[Safe rule]`
2. The Flat layer draws as today. `[Must-match]`
3. In the Upright layer, the backdrop pixels of group hex B draw as one
   draw item keyed `(layer 2, primary B, secondary 0xFFFFFFFF)`. No object
   key reaches that secondary, since a list index is below 2^29 and a part
   below 5 (SPEC-022 §2.1), so every existing key and its order are kept.
   `[Must-match]` (an amendment of SPEC-022 §2.1)
4. A group item draws only its own pixels, each opaque unless the fade rule
   applies (section 3.6), clipped to the refresh rectangle. Groups of hexes
   outside the visited lattice of SPEC-014 §3.44 still draw. `[Safe rule]`
5. Roof, Outline, FloatingText, NoHex and Cursor draw as today, over the
   backdrop. `[Must-match]`
6. Suppressed objects (section 3.8) are not drawn, and take no list
   position change: list indexes keep counting them, as hidden objects
   count today. `[Must-match]`
7. Translucent objects blend with whatever the canvas holds when they draw,
   which under this algorithm includes the backdrop groups ordered before
   them. `[Must-match]`

### 3.6 Foreground fade

1. A pixel is a foreground pixel when its fade bit is set and D ≥ 1.
   `[Safe rule]`
2. **Player.** When the player is placed and not hidden, a foreground pixel
   whose group hex B is greater than the player's hex, inside the `egg.frm`
   rectangle placed as SPEC-014 §3.37 places it, is drawn through the mask
   with that section's compositing formula and weight m at the pixel.
   Under the Vanilla preset this is the only fade. `[Must-match]` for the
   formula, `[Safe rule]` for the selection
3. **Cursor target (enhanced preset).** With `roof_peek = "cursor_xray"`, or
   while Alt is held, a foreground pixel whose group hex is greater than
   the hex under the cursor, inside the SPEC-022 §3.10 peek disc and ring,
   is dissolved by the same ordered dither the roof peek uses. `[Safe rule]`
4. When `egg.frm` cannot be loaded, foreground pixels draw opaque (the
   SPEC-014 §3.37 `[May-improve]` rule). `[Safe rule]`
5. The fade never changes which pixels belong to which group, so it never
   reorders anything. `[Safe rule]`

### 3.7 Light

1. A pixel with D ≥ 1 is lit as an object on hex B: level
   `trunc(max(ambient, light(B)) / 512)` through the SPEC-001 §3.7 intensity
   table; indexes 229-255 stay unlit. `[Safe rule]`
2. A pixel with D = 0 is lit by the hex under its canvas point (SPEC-003
   §3.7), by the same rule. `[Safe rule]` (question 3 asks about the
   interpolated floor light)
3. The light read is the SPEC-022 §3.8 buffer the objects use, so a light
   change relights the backdrop on the next refresh. `[Must-match]`

### 3.8 Blocking, roofs and interaction

1. Blocking, sight, line of fire, doors and exit grids come only from map
   objects and their flags, unchanged (SPEC-007 §3.1-§3.2). A painted wall
   is backed by ordinary wall or scenery objects that stay on the map.
   `[Must-match]`
2. Suppression `1` stops drawing every object of art category 3 (walls) on
   that elevation; `2` stops drawing the listed PIDs; `3` both. A suppressed
   object keeps every world role: it blocks, stops sight, is saved, and is
   seen by scripts. `[Safe rule]`
3. Doors, critters, items and every object not suppressed draw as today, so
   doors open and close visibly over the painted frame. `[Must-match]`
4. Roof squares stay tiles in the Roof layer; roof hiding and the roof peek
   are unchanged. A backdrop never holds roofs. `[Must-match]`
5. Picking reads the world (SPEC-022 §3.13 item 3). A sprite pixel that a
   group with a larger hex covers in the final canvas is not a pick hit for
   that sprite. `[Safe rule]`
6. Suppressed objects are never outlined and never pick-hit. `[Safe rule]`

### 3.9 Both paths

1. The instanced path draws the same canvas, byte for byte: colour and fade
   as `R8Uint` textures, depth as `R16Uint`, read with `textureLoad` at
   integer coordinates (SPEC-022 §3.11). `[Must-match]`
2. The GPU may realise section 3.5 with a depth or stencil test, or with
   one instance per chunk that compares the group hex, if every canvas byte
   equals the reference path's. `[May-improve]`
3. A refresh the GPU cannot express is refused and redrawn by the reference
   path (SPEC-022 §3.12). `[Safe rule]`

### 3.10 Memory

1. A decoded chunk at S = 256 with all planes is 64 KiB colour, 128 KiB
   depth and 8 KiB fade. `[Safe rule]`
2. Chunks are decoded on demand for the refresh rectangle and kept in a
   cache of at most 64 MiB (about 320 such chunks); eviction is
   least-recently-drawn, ties by ascending (elevation, row, column). The
   cache never changes a canvas byte. `[May-improve]` for the policy,
   `[Must-match]` for the output
3. Each chunk file is read with the 64 MiB asset bound (`MAX_ASSET_BYTES`).
   `[Safe rule]`

## 4. Validation and errors

`BackdropIndex::parse` and `BackdropChunk::parse` return `ParseError` with
format "BKD" or "BKC" and the byte offset, and decode nothing more, for:

1. a wrong magic or a version other than 1; `[Safe rule]`
2. an empty elevation mask, a record whose elevation is not its mask bit,
   or the colour plane bit clear; `[Safe rule]`
3. a chunk size not in {64, 128, 256, 512}, a width or height of 0 or above
   16384, or chunk counts that do not equal `ceil(W / S)` and
   `ceil(H / S)`; `[Safe rule]`
4. an origin hex outside 0-39999, a PID count above 4096, a compression
   code above 1, or a nonzero reserved byte; `[Safe rule]`
5. a chunk whose elevation, planes, column or row disagree with the index,
   or whose decoded payload length is not exact; `[Safe rule]`
6. a depth value of 40001 or more; `[Safe rule]`
7. a zlib stream that does not end exactly at the payload length, or bytes
   after it. `[Safe rule]`

At run time:

8. A missing or malformed `.bkd`, or a palette hash that differs from the
   loaded palette's, is one diagnostic per map, and the map draws as
   without a backdrop (tiles and every object drawn). `[Safe rule]`
9. A present chunk that is missing or malformed disables that elevation's
   backdrop for the session, with one diagnostic, so no hole opens where
   suppressed walls were. `[Safe rule]`
10. No input makes either path panic, read out of bounds, or loop without
    bound. `[Safe rule]`

## 5. Determinism property

1. A backdrop changes no world value: `state_hash`, `combat_hash`, the
   checkpoint stream, saves and the RNG are the same with or without it
   (SPEC-022 §3.13). `[Must-match]`
2. The canvas is a function of the map data, the world state, the logged
   input and the shell clock; the cache and decode timing never change it.
   `[Must-match]`
3. A replay digest covers the canvas, so a backdrop changes the digest of
   maps that ship one, identically on both paths. A map without a `.bkd`
   gives every existing digest and gallery image unchanged. `[Must-match]`
4. Decoding happens off the simulation step; a chunk not yet decoded when a
   refresh needs it is decoded synchronously, never skipped. `[Must-match]`

## 6. Edge cases

- A backdrop smaller than the map: outside it, tiles draw normally.
- Colour 0 with D ≥ 1: transparent, and the pixel belongs to no group.
- Two elevations with backdrops and one without.
- A suppressed PID that is a door (question 6).
- The player on hex B of a foreground group: B is not greater than the
  player's hex, so those pixels draw opaque, and since the group draws after
  the objects of its own hex they cover the player there. Authors therefore
  give a wall's pixels the hex of its own blocking wall object, which no
  critter can occupy.
- Zoom 0.5 on a 1920 by 1080 window: about 8 million canvas pixels and up
  to 140 chunks at S = 256.
- A script that moves a wall object: its suppression follows the object; the
  painted pixels stay where they are.

## 7. Synthetic test vectors

All with a synthetic palette in which indexes 7 and 9 map to themselves at
intensity level 128, ambient light 65536 and no light source, so lighting
leaves indexes unchanged.

Vector A, the 4×4-hex patch. The patch is the 16 hexes of columns 100-103
and rows 100-103 (hexes 20100-20103, 20300-20303, 20500-20503, 20700-20703).
The backdrop's depth plane holds 0 over the pixels of rows 100-101 and
`20504` (group hex 20503) over the pixels of rows 102-103: a two-level depth
step. One upright 2 × 2 sprite of index 9, unlit, is placed:

| Sprite on hex | Against the row 102-103 pixels | Against the row 100-101 pixels |
| ---: | --- | --- |
| 20301 (row 101) | Covered: the backdrop shows (20301 < 20503) | Sprite shows |
| 20701 (row 103) | Sprite shows (20701 > 20503) | Sprite shows |
| 20503 (the group hex) | Covered: ties go to the backdrop | Sprite shows |

Vector B, exact bytes. A backdrop of 4 by 1 pixels at canvas (100, 100),
colour `07 07 07 07`, depth `0000 0000 501D 501D` (D = 20509, group hex
20508). A 4 × 1 upright sprite `09 09 09 09` drawn at (100, 100):

- object hex 20100: canvas `09 09 07 07`;
- object hex 20508: canvas `09 09 07 07`;
- object hex 20509: canvas `09 09 09 09`;
- no object: canvas `07 07 07 07`.

Vector C, sort keys. The group of hex 20508 (`0x501C`) has key
`(2 << 61) | (20508 << 32) | 0xFFFFFFFF` = `0x4000501CFFFFFFFF`. An object on
hex 20508, list index 0, part 0 has `0x4000501C00000000`; on hex 20509,
`0x4000501D00000000`. They sort: object on 20508, group 20508, object on
20509.

Vector D, fade. As vector B, with the fade bit set on all four pixels and
the sprite being the player's own, on hex 20100, its egg rectangle covering
(100, 100)-(103, 100) with weight m = 1 there. The two D ≥ 1 pixels draw
through the mask: each becomes `mix[intensity[07][1]][intensity[09][127]]`;
the two D = 0 pixels stay `09` (the player is drawn over them). With the
player on hex 20600 (above the group hex) they draw opaque: `07`.

Vector E (refused): magic `VBKE`; chunk size 100; a depth value 40001; a
chunk whose payload decodes to one byte short; a `.bkd` whose palette hash
differs (map draws without the backdrop, one diagnostic).

## 8. Open questions for the owner

1. **Ties.** Should a backdrop group draw after the objects of its own hex
   (as proposed) or before them? Recommended: after, so the key needs only
   the free secondary `0xFFFFFFFF` and no existing key changes.
2. **Key amendment.** Is reserving secondary `0xFFFFFFFF` in layer 2 for the
   backdrop group acceptable as a SPEC-022 §2.1 amendment? Recommended: yes.
3. **Floor light.** Should D = 0 pixels use the interpolated floor light of
   SPEC-022 §3.9 instead of per-hex light? Recommended: per-hex in revision 1;
   interpolated light as a later revision once the square under a pixel is
   specified.
4. **LZSS.** Should chunks also allow the DAT LZSS framing? Recommended: no;
   stored and zlib only, both already decoded by `vault-formats`.
5. **Palette mismatch.** Refuse the backdrop, or draw it anyway with a
   warning? Recommended: refuse (draw without it), since wrong indexes are
   wrong colours.
6. **Suppressing doors.** May a listed PID be a door? Recommended: no; a
   door PID in the list is a diagnostic and the door still draws.
7. **Vanilla preset.** Should backdrops draw under the Vanilla preset?
   Recommended: yes, with the player egg only, since a mod that ships one has
   no other floor art.
8. **Cache budget.** Is 64 MiB of decoded chunks the right default?
   Recommended: yes, with a `backdrop_cache_mib` setting for small devices.
9. **Fade weight.** One fade bit, or an 8-bit weight per pixel?
   Recommended: one bit; the egg mask supplies the weight.

## 9. Out of scope

- 3D geometry, normal maps or per-pixel light sources.
- Animated backdrops (palette cycling of indexes 229-255 still works).
- Roof backdrops.
- Authoring tools for backdrops and their depth images.
- Changing how blocking, sight or roofs are computed.

## 10. Owner decisions and what was built (2026-11-27)

The nine questions of section 8 take their recommended answers. SPEC-022
Revision 5 (section 3.17) is the layout and the rules as built; where it
differs from this brief, it is right. The differences:

- The floor pass has its own key, (layer 0, primary 10000, secondary 0);
  section 3.5 gave it none. The group key of section 3.5 item 3 needed the
  key rules amended to accept it.
- The elevation records all come first, then each elevation's presence
  table and PID list (section 3.2 could be read either way). A PID count is
  0 unless the suppression code is 2 or 3, and nothing follows the last list.
- The instanced path builds per-item images on the CPU and uses no depth
  attachment and no 16-bit texture (section 3.9 item 2 allows it), so
  section 3.9 item 1's `R16Uint` texture is not used.
- A faded pixel inside the player mask is lit without the 229-255
  exemption, as a masked wall object is (section 3.7 item 1 holds outside
  the mask). A D = 0 pixel off the grid is lit at the ambient level.
- Picking follows the drawing order, ties included: section 3.8 item 5's
  "larger" is "at or past". A flat object's pixel is covered by any group.
- The cursor fade follows the roof peek whenever a refresh has one.
- Diagnostics are log lines, not on-screen status text.
- An object whose art has no file name is now logged and not drawn on the
  platform, where before it stopped the render.
