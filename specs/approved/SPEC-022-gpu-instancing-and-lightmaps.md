# SPEC-022: GPU-Instanced Sprite Batching and Hex Lightmaps

**Status:** approved by the Spec-Author sub-agent (Team A), 2026-10-27, under AGENTS.md "Spec Authoring (Team A)". Revision 3 (Team A, 2026-10-30); see "Revision history".

**Provenance.** Author: the Spec-Author sub-agent (Team A), 2026-10-27.
Sources:

- the owner's brief for this spec, relayed by the coordinator on 2026-10-27:
  1. a GPU-instanced sprite batching and hex-lightmap pipeline for
     `vault_render` (`vault_render::layers` and `vault_render::map::enhanced`)
     that keeps 100% pixel parity with the existing 8-bit indexed palette
     renderer (`vault_formats::palette`) by default, while reducing draw
     calls through instanced sprite quads;
  2. a deterministic sort order for sprite instances across floor tiles, flat
     scenery, standing objects and critters, roof tiles (`vault_core::roof`)
     and floating text, so that batching never changes occlusion or Z order;
  3. a per-hex light intensity buffer (the 200-by-200 hex grid, ambient and
     point-light falloff, clamped to `[0, 65536]`) and the radial roof X-ray
     peek mask parameters, both passed to the GPU layer pass;
  4. `cargo xtask capture-gallery && git diff --exit-code docs/assets` stays
     byte-identical in the default parity mode, and headless render tests use
     synthetic fixtures in `tests/fixtures/spec022/`;
  5. every rule tagged `[Must-match]`, `[May-improve]` or `[Safe rule]`, and
     every open question recorded in a Decisions section as an owner
     instruction or a Team A default;
- the project's approved specs:
  - SPEC-001: palette layout and transparency (sections 2.1 and 3.1), palette
    cycling (section 3.2), sprite placement (section 3.5), and the intensity
    and mix tables (section 3.7);
  - SPEC-002 section 3.2: the per-color blend tables;
  - SPEC-003: flag words (section 2.6), the floor and roof tile words
    (section 2.8), and the hex grid and projections (sections 3.4 to 3.7);
  - SPEC-006 section 3.6: ambient and local light;
  - SPEC-007: the art identifier (section 2.1) and the animation steps
    (sections 3.10 and 3.11) that reinsert objects;
  - SPEC-014: roof hiding (section 3.36), the player cutout (section 3.37),
    the draw order, lighting, outlines and floor light (section 3.44), and
    floating text (section 3.46);
  - SPEC-020: the session runtime, and its rule for re-recording snapshots
    (section 3.4);
  - SPEC-021: the FNV-1a 64 hash (section 2.1), used here as a fixture
    digest;
- the project's documentation of its renderer and gallery in
  `docs/REFERENCE.md`: the documentation gallery, the enhanced viewport, the
  roof peek, and the map view notes;
- public documentation of the wgpu and WebGPU APIs: integer texture formats,
  `textureLoad`, scissor rectangles, texture copies, instanced draws, and the
  primitive-order guarantee for color writes;
- the 4-by-4 ordered-dither (Bayer) matrix, a public, standard construction.

No decompilation, disassembly or re-implementation source was consulted, no
game asset was opened, and no engine Rust code was read. Engine modules are
named here only by the module paths the brief and the project documentation
give. Team A's general knowledge comes from model training data whose sources
cannot be audited (see `PROVENANCE.md`, "In-session spec authoring"). This
spec adds a second way to draw the world canvas. It adds no game rule.

**Provenance of revision 2.** Author: the Spec-Author sub-agent (Team A),
2026-10-27, on the coordinator's request (Wave 4) relaying the Wave 3
implementer's parity findings. Sources: the implementer's report as relayed
by the coordinator, recorded below as **measured by the implementer, Wave 3**
(the reference path's roof X-ray rule; the three reference draws that no key
slot or mode could hold; the reference square order; the per-elevation object
lists; whole-view refreshes); the coordinator's later update with the
vault-render implementer's measurement of the blend gray levels (palette
gray levels never exceed 7; only a hand-built ink can); the owner's
confirmation, in the Wave 3 prompt, of decisions D-7 and D-11; SPEC-003
section 3.6 (the floor and roof square corners) and SPEC-014 section 3.44
(the outline list and the gray levels). No engine Rust code was read for this
revision either. Every revision 2 vector was computed from this text by a
throwaway script that is not committed, after the same script had reproduced
every revision 1 vector from the revision 1 text.

**Provenance of revision 3.** Author: the Spec-Author sub-agent (Team A),
2026-10-30, on the owner's brief of 2026-10-30, "Lifting the 4 Renderer
Refusals". Sources:

- the owner's brief, which names this work revision 2. Revision 2 was already
  approved, so this is revision 3 (D-33);
- `docs/REFERENCE.md`, "Renderer notes for SPEC-022 revision 2", in which the
  implementer states what the reference path does, as read and measured on
  2026-10-30, recorded below as **measured by the implementer, 2026-10-30**;
  and its "Render path" section, which lists the refusals;
- this spec's revision 2, SPEC-003 section 3.6 and SPEC-014 sections 3.37 and
  3.44.

No engine Rust code was read for this revision either, and no decompilation,
disassembly, re-implementation source or game asset was used. Every revision 3
vector was computed from this text by a throwaway script that is not
committed, then checked by a second, separate script from the written files
alone (section 7).

### Revision history

- **Revision 1 (2026-10-27).** First approved text.
- **Revision 2 (2026-10-27).** Adopts the five Wave 3 findings, measured by
  the implementer on the reference path:
  1. **Roof peek.** Section 3.10 now states the reference path's X-ray rule:
     a whole-pixel center at the cursor's canvas pixel, radii `round(96 /
     zoom)` and `round(48 / zoom)` in canvas pixels, an inclusive inner disc,
     and the ring test `256·d² ≥ (16·inner + (b + 1)·ring)²`. The uniform
     block keeps its layout; its peek fields change units (section 2.4). The
     revision 1 rule differed from the reference by 91 to 550 pixels per case
     on the showcase map.
  2. **Unslotted draws.** The enhanced shroud gets a slot after every roof
     square (layer 3, primary 10000) and a new read mode, `remap` (code 6).
     Ground-item highlights take outline positions from 100 up. The cutout of
     an unlit scene is the `egg` mode with light source `none` (sections 2.1,
     2.2, 3.3 and 3.6).
  3. **Square order.** Floor and roof squares are ranked by (corner y, corner
     x, square index), not by square index (section 3.4, D-10).
  4. **Lists and refreshes.** The list index counts only the current
     elevation's entries, and the reference path always redraws the whole view
     (sections 2.1 and 3.2).
  5. **Blend gray bound.** The gray level that selects a blend-table row is
     clamped to 0..=7 on both paths (section 3.6, `[May-improve]`).
     Palette-derived gray levels never exceed 7, so no output changes; the
     bound guards hand-built inks only.
  - The owner confirmed D-7 and D-11; they are now owner instructions.
  - The 64-bit key layout, every revision 1 key, the light buffer, the batch
    rules and the instance record are unchanged. New and changed vectors are
    in `tests/fixtures/spec022/rev2/` (section 3.15).
- **Revision 3 (2026-10-30).** Closes the gap between revision 2 and the
  reference path as the implementer measured it, so that the follow-up can
  lift the four refusals (section 3.16).
  1. **Numbering.** Two numbers now equal the reference path's draw log. The
     square rank is the position among all 10000 squares, and a highlight's
     outline position is `n + k` for a list holding n entries. Neither changes
     the draw order or a canvas byte (sections 2.1, 3.3 and 3.4; D-34, D-35).
  2. **Enhanced shroud.** Section 3.3 states the measured rule: one constant
     dark index, written over D without reading it. The slot and the `remap`
     mode stay (D-36).
  3. **Unlit scenes.** Section 3.3 now gives every item of an unlit scene its
     mode. The translucency bits are ignored, and only the cutout blends.
  4. **Roof peek.** Section 3.10 adds the tile rule for the center, double
     precision, and the kept pixel's draw. Its safe bounds, which the
     reference path does not have, now refuse the refresh when they change a
     value (D-38).
  5. **Refreshes and lists.** Each refresh starts from a new canvas of index 0
     (section 3.2). Hidden objects, the in-world cursor and its AP text count
     in the list index (section 2.1).
  6. **Follow-up.** Section 3.16 is the checklist that lifts the four
     refusals, with the parity test that proves each one.
  - The 64-bit key layout, the instance record, the draw modes, the light
    buffer, the uniform block, the batch rules, the peek test and the gray
    bound are unchanged. New vectors are in `tests/fixtures/spec022/rev3/`.
    Team A changed no revision 1 or revision 2 file.

## Clean-Room Rules for Research Agents
- Document ONLY binary layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from any inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

The engine draws the map view into an 8-bit indexed **world canvas**, one
palette index per pixel. The display stage later resolves the canvas through
the display palette (SPEC-001). This spec calls the existing CPU compositor
of `vault_render::map` and `vault_render::layers` the **reference path**.

This spec defines a second path, the **instanced path**. It draws the same
canvas on the GPU with instanced sprite quads. It defines:

1. the frame layers and the 64-bit **sort key** that orders every sprite
   instance (sections 2.1 and 3.4);
2. the logical **sprite instance** and its seven **draw modes**, as exact
   integer formulas over the lookup tables the reference path already uses
   (sections 2.2 and 3.6);
3. the **batch plan**: which instances share one draw call, and how draws
   that read the canvas are isolated (section 3.7);
4. the **hex light buffer**: the per-hex light of the current elevation,
   clamped to `[0, 65536]` (sections 2.3 and 3.8);
5. the **layer uniform block**, carrying the ambient light, the player
   cutout rectangle and the **roof peek** parameters (sections 2.4 and 3.10);
6. how the path is selected, when it falls back to the reference path, and
   what it must never change (sections 3.1, 3.12 and 3.13);
7. the tests the implementer writes, and synthetic vectors
   (sections 4 and 5, and `tests/fixtures/spec022/`).

It does not change the reference path's output, the display stage, palette
cycling, picking, the simulation, saves, input logs or replays.

**Compatibility tags.** As in SPEC-020, the tags compare the engine with **its
own behavior**, here the reference path's, not with the original game:

- `[Must-match]`: the instanced path's canvas equals the reference path's
  canvas byte for byte, for the same world, view and refresh rectangle. The
  key, batch, light, uniform and peek values of this spec, and their
  fixtures, are reproduced exactly.
- `[May-improve]`: an implementation freedom, such as a buffer layout,
  caching, or doing a CPU step on the GPU. It is allowed only if no canvas
  byte and no value that this spec fixes changes.
- `[Safe rule]`: how the instanced path handles a state it cannot draw
  exactly: no adapter, a lost device, a value out of range or a texture
  limit. It refuses with a stated error and draws that refresh with the
  reference path. It never panics, reads out of bounds, or draws a guessed
  pixel.

**Notation.** Canvas pixel (x, y) has x increasing rightward and y downward,
from (0, 0) at the canvas's top-left. A rectangle `(x0, y0, x1, y1)` is
half-open: it holds x0 ≤ x < x1 and y0 ≤ y < y1. "Hex" is SPEC-003's linear
hex number `column + 200 × row`. A "square" is a floor or roof grid position
`column + 100 × row` (SPEC-003 section 2.8). The intensity table I, the
additive mix table X and the blend tables are those of SPEC-001 section 3.7
and SPEC-002 section 3.2: `I[i][k]` is row i, column k. Integer division
truncates, and `>>` is a logical right shift. Keys are written as 16
uppercase hexadecimal digits.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Frame Layers and the Sort Key

A refresh draws these layers, in this order (SPEC-014 section 3.44, "Frame
layers"). The layer number is the top three bits of the sort key. The table
is one rule. `[Must-match]`

| Layer | Name | Contents | Primary field | Secondary field |
| ---: | --- | --- | --- | --- |
| 0 | Floor | Floor squares | Square rank (section 3.4) | Part (always 0) |
| 1 | Flat | Objects of the flat pass | Hex number | 8 × list index + part |
| 2 | Upright | Objects of the upright pass: standing objects, critters, walls and other non-flat scenery | Hex number | 8 × list index + part |
| 3 | Roof | Drawn roof squares; then the enhanced shroud (revision 2) | Square rank; 10000 for the shroud | Part; the shroud draw rank for the shroud |
| 4 | Outline | Outline rings, in outline-list order; then ground-item highlights (revision 2) | 0 | 8 × outline position + part |
| 5 | Floating text | Floating text boxes, in creation order | 0 | Creation rank |
| 6 | No-hex | Objects that have no hex | 0 | Reference draw rank |
| 7 | (Reserved) | Never used; an item of layer 7 is refused | — | — |

The 64-bit sort key is: `[Must-match]`

| Bits | Width | Field |
| --- | ---: | --- |
| 61-63 | 3 | Layer |
| 32-60 | 29 | Primary |
| 0-31 | 32 | Secondary |

`key = layer × 2^61 + primary × 2^32 + secondary`. Instances are drawn in
ascending key order, and no two instances of a refresh have the same key.

Field domains: `[Must-match]`

- **Part**, in layers 1 to 3, is 0 to 4. Part 0 is the whole sprite, or the
  part drawn through the player cutout. Parts 1 to 4 are the top, bottom,
  left and right strips around the cutout (section 3.5). Layer 0 uses only
  part 0.
- **Part**, in layer 4, is 0 for the ring and 1 for the wrap column
  (section 3.3).
- **List index** is the zero-based position of the object in its hex's object
  list (SPEC-014 section 3.44, "Per-hex object lists"), counting only the
  entries of the **current elevation**: the engine keeps a separate list per
  elevation (measured by the implementer, Wave 3; revision 1 counted every
  elevation's entries). The draw order is the same under either count. It is
  below 2^29. Hidden objects count, though they are not drawn, and the
  in-world cursor and its AP text take two places, at the cursor's place in
  the list (measured by the implementer, 2026-10-30).
- **Hex number** is 0 to 39999. **Square rank** is 0 to 9999.
- **Shroud.** Primary 10000 is used only by the enhanced shroud (section
  3.3), so every shroud draw follows every roof square. Its secondary is the
  **shroud draw rank**: the position of the draw among the reference path's
  shroud draws of the refresh, from 0. It is below 2^32. The reference path
  makes one shroud draw per refresh, so the rank is 0 in play.
- **Outline position** is a running number (revision 3). If the outline list
  holds n entries, n ≤ 100, its entries have positions 0 to n − 1, in list
  order, and the k-th ground-item highlight (k from 0) has position `n + k`.
  This is the number the reference path's draw log gives it (measured by the
  implementer, 2026-10-30). Revision 2's `100 + k` agreed only when the list
  was full, and is withdrawn (D-35). The key order is the same under either
  number. A position is below 2^29.
- **Creation rank** is the text's position among the live floating texts,
  oldest first. **Reference draw rank** is the position in which the
  reference path draws the object among the no-hex objects.

These domains keep the revision 1 key layout and every revision 1 key: the
shroud uses a primary that revision 1 left unused, and the highlights use
positions that the outline list does not hold.

### 2.2 The Sprite Instance

An instance is one rectangle of one image, drawn with one draw mode. These
are its logical fields. `[Must-match]`

| Field | Type | Meaning |
| --- | --- | --- |
| key | u64 | Sort key (section 2.1) |
| mode | enum | Draw mode, table below |
| flags | u8 | Bit 0 `EXEMPT`: indexes 229-255 are left unlit. Bit 1 `PEEK`: the roof peek test applies. Other bits are 0 |
| light source | enum | `none` (0), `hex` (1), `ambient` (2), `fixed` (3), `level image` (4) |
| hex | u32 | For light source `hex`: the hex whose light is used |
| fixed light | u32 | For light source `fixed`: the light value, 0 to 65536 |
| dst x, dst y | i32 | Canvas position of the rectangle's top-left pixel |
| width, height | u16 | Rectangle size, 1 or more in each axis |
| image, src x, src y | — | The source image, and the image pixel drawn at (dst x, dst y) |
| level image, level x, level y | — | For light source `level image`: the image of light levels, and its pixel at (dst x, dst y) |
| clip | rect | The scissor rectangle (section 3.2) |
| aux | u16 | Bits 0-2: blend table, 0 to 4. Bits 8-10: gray level, 0 to 7, after the clamp of section 3.6 (draw mode 5 only). Other bits are 0 |
| remap table | — | For draw mode 6 only (revision 2): an image 256 entries wide and N rows high, 1 ≤ N ≤ 255. Row c − 1 is the function applied for source value c |

The draw modes are one rule. "Reads the canvas" means that the result depends
on the canvas pixel D under the instance pixel. `[Must-match]`

| Code | Mode | Light source | Reads the canvas | Used for |
| ---: | --- | --- | --- | --- |
| 0 | `plain` | `none` | No | Opaque outlines, floating text, and every item of an unlit scene that is not drawn through the cutout (revision 3) |
| 1 | `lit` | `hex`, `ambient` or `fixed` | No | Lit floor squares with one light, objects, roofs, strips |
| 2 | `floor_map` | `level image` | No | Floor squares lit per pixel |
| 3 | `translucent` | `hex` or `fixed` | Yes | Objects with exactly one translucency bit |
| 4 | `egg` | `hex`, `ambient`, or `none` (revision 2: an unlit scene) | Yes | The parts of objects and roofs drawn through the player cutout |
| 5 | `outline_blend` | `none` | Yes | Translucent outlines |
| 6 | `remap` | `none` | Yes | The enhanced shroud (revision 2) |

The five blend tables are numbered as SPEC-014 section 3.44 lists the
translucency colors. `[Must-match]`

| Blend table | Foreground color | Selected by |
| ---: | --- | --- |
| 0 | Wall translucency, key 25439 [33] | Basic flag `0x00010000`; outline type 4 |
| 1 | Glass translucency, key 10239 [35] | Basic flag `0x00020000`. Table 1 alone uses the glass gray table |
| 2 | Steam translucency, key 32767 [220] | Basic flag `0x00040000` |
| 3 | Energy translucency, key 30689 [58] | Basic flag `0x00080000` |
| 4 | Red translucency, key 31744 [133] | Basic flag `0x00004000`; outline types 2 and 16 |

**Recommended GPU record.** An implementation may upload instances in this
48-byte little-endian record. The key stays on the CPU. `[May-improve]`

| Offset | Bytes | Field |
| --- | ---: | --- |
| `0x00` | 4 | i32 dst x |
| `0x04` | 4 | i32 dst y |
| `0x08` | 2 | u16 width |
| `0x0A` | 2 | u16 height |
| `0x0C` | 2 | u16 src x, in the atlas page |
| `0x0E` | 2 | u16 src y, in the atlas page |
| `0x10` | 4 | u32 atlas page |
| `0x14` | 1 | u8 mode |
| `0x15` | 1 | u8 flags |
| `0x16` | 1 | u8 light source |
| `0x17` | 1 | Reserved, 0 |
| `0x18` | 4 | u32 hex |
| `0x1C` | 4 | u32 fixed light |
| `0x20` | 4 | u32 aux |
| `0x24` | 2 | u16 level x, in the level page |
| `0x26` | 2 | u16 level y, in the level page |
| `0x28` | 4 | u32 level page |
| `0x2C` | 4 | Reserved, 0 |

For draw mode 6, the level x, level y and level page fields locate the remap
table instead of a level image: its row c − 1 starts at (level x, level y +
c − 1). `[May-improve]`

### 2.3 The Hex Light Buffer

The buffer holds the local light of every hex of the current elevation.
`[Must-match]`

| Property | Value |
| --- | --- |
| Logical content | 40000 unsigned 32-bit values, entry h for hex h |
| Value | `clamp(local light of hex h, 0, 65536)` (section 3.8) |
| GPU form | A 200-by-200 `R32Uint` texture. Texel (x, y) holds the entry of hex `x + 200 × y`: x is the hex column, y the hex row |
| Byte image | The 40000 values as little-endian u32, in hex order: 160000 bytes, entry h at byte `4 × h` |
| Digest | FNV-1a 64 (SPEC-021 section 2.1) of the byte image, written as 16 uppercase hexadecimal digits |

The buffer holds only the local light. The ambient light travels in the
uniform block, and the two are combined in the shader (section 3.8). An
implementation may use a storage buffer with the same byte image in place of
the texture. `[May-improve]`

### 2.4 The Layer Uniform Block

One block per refresh, 128 bytes, little-endian. The layout is WGSL
uniform-compatible: the threshold table is `array<vec4<u32>, 4>`, which has
the same bytes as 16 consecutive u32. `[Must-match]`

| Offset | Bytes | Type | Field |
| --- | ---: | --- | --- |
| `0x00` | 4 | u32 | Canvas width, in pixels |
| `0x04` | 4 | u32 | Canvas height |
| `0x08` | 4 | i32 | OX: the canvas's shared translation (SPEC-003 section 3.6) |
| `0x0C` | 4 | i32 | OY |
| `0x10` | 4 | u32 | Ambient light, 0 to 65536 |
| `0x14` | 4 | u32 | Current elevation, 0 to 2 |
| `0x18` | 4 | u32 | Flags. Bit 0: the cutout mask is bound. Bit 1: the roof peek is active. Other bits are 0 |
| `0x1C` | 4 | u32 | Reserved, 0 |
| `0x20` | 4 | i32 | Cutout left: canvas x of mask pixel (0, 0) |
| `0x24` | 4 | i32 | Cutout top: canvas y of mask pixel (0, 0) |
| `0x28` | 4 | u32 | Cutout mask width (129 for `egg.frm`) |
| `0x2C` | 4 | u32 | Cutout mask height (98 for `egg.frm`) |
| `0x30` | 4 | i32 | Peek center x: the canvas column of the cursor's pixel (section 3.10) |
| `0x34` | 4 | i32 | Peek center y: the canvas row of the cursor's pixel |
| `0x38` | 4 | u32 | Peek outer radius, `inner + ring`, in whole canvas pixels |
| `0x3C` | 4 | u32 | Peek inner radius, in whole canvas pixels |
| `0x40` | 64 | 16 × u32 | Peek ring thresholds `Q[b] = (16 × inner + (b + 1) × ring)²`, entry b at `0x40 + 4 × b` |

Revision 2 keeps the offsets and types and changes the units of `0x30` to
`0x7F`: revision 1 held 1/16-pixel values of its own peek rule. The
revision 1 bytes of the section 5.4 scene are superseded by
`rev2/scene-uniforms.hex`. `[Must-match]`

When the cutout is not active, bit 0 is clear and offsets `0x20` to `0x2F` are
zero. When the peek is not active, bit 1 is clear and offsets `0x30` to
`0x7F` are zero. `[Must-match]`

## 3. Behavioral Rules & Edge Cases

### 3.1 Render Paths and Their Selection

1. The engine has two render paths for the world canvas: `reference` (the
   existing CPU compositor) and `instanced` (this spec). The setting is
   `render_path`, beside `preset` and `viewport_mode` in the shell settings,
   with the command-line override `--render-path reference|instanced`. It is
   read under every preset and game profile, because it changes no output.
   `[Must-match]`
2. The default is `reference`, in the desktop shell, in replays and in
   `cargo xtask capture-gallery`, and `instanced` is opt-in (Decision D-7,
   an owner instruction since revision 2). `[Must-match]`
3. The instanced path has one lighting mode in revisions 1 to 3, **parity**: its
   canvas is the reference path's, byte for byte. No other mode exists, and
   none may be reached by default (Decision D-20). `[Must-match]`
4. Parity is defined on the indexed canvas. The display stage after it
   (palette resolve, palette cycling, gamma, scaling, the interface frame,
   the enhanced view's dimming) is unchanged and is shared by both paths. An
   implementation that resolves the palette on the GPU must produce exactly
   the colors the display stage produces for the same canvas.
   `[Must-match]`

### 3.2 The Refresh Pipeline

The reference path draws the canvas in **refreshes**: each refresh redraws
one rectangle R of the canvas. The instanced path runs the same refreshes,
with the same rectangles, in the same order, on a canvas that it keeps on the
GPU. `[Must-match]`

**Whole-view refreshes (revision 2).** The reference path always redraws the
whole view: R is the whole canvas of the view (measured by the implementer,
Wave 3). The enhanced view's canvas tiles are separate views, each with its
own canvas and its own whole-view refreshes, and a sprite that crosses a seam
draws in both tiles. So in play every refresh has R equal to the canvas, and
every instance of a refresh has the same clip. The rules below still hold for
any R, and the synthetic scene of section 5.4 keeps a partial R to test the
executor's clip handling. `[Must-match]`

**One refresh, in order (revision 3).** The reference path starts each refresh
from a new canvas of index 0 and keeps no pixel from the last one (measured by
the implementer, 2026-10-30). It then draws, in this order: `[Must-match]`

| Step | Draw | Layer |
| ---: | --- | ---: |
| 1 | A new canvas of index 0 | Base fill |
| 2 | The floor squares | 0 |
| 3 | The flat pass, then the upright pass | 1, 2 |
| 4 | The roof squares | 3 |
| 5 | The enhanced shroud | 3 |
| 6 | The outline list, at most 100 entries | 4 |
| 7 | The ground-item highlights | 4 |
| 8 | The floating text | 5 |
| 9 | The objects without a hex | 6 |

For each refresh R:

1. **Base fill.** Fill R with index 0, as the reference path's new canvas
   does (revision 3). The off-grid shroud is not a base fill: it is drawn in
   layer 3. A synthetic executor test may name another fill, as sections 5.4
   and 5.6 do. `[Must-match]`
2. **Gather.** Build the draw items of layers 0 to 6 for R from the same
   world state, view and visibility rules the reference path uses
   (section 3.3). `[Must-match]`
3. **Key and sort.** Give each instance its key (section 3.4) and sort the
   instances by ascending key. `[Must-match]`
4. **Cull.** Drop every instance whose rectangle, clipped to its clip,
   is empty. `[Must-match]`
5. **Plan.** Split the sorted instances into batches (section 3.7).
   `[Must-match]`
6. **Execute.** Draw the batches in order (section 3.7). `[Must-match]`

**Clip.** Every instance of layers 0, 1, 2, 3, 5 and 6 (the shroud
included) has clip R. Every
instance of layer 4 has the whole canvas as its clip, because an outline
ring may reach up to 2 pixels outside R (SPEC-014 section 3.44, "Outline
geometry", clipping) and its image already holds that rule (section 3.3).
`[Must-match]`

If the reference path draws anything else into the canvas between the base
fill and the end of layer 6 (an enhanced overlay drawn under the roofs, for
example), the implementer expresses it as instances of the matching layer
with a draw mode of section 3.6, at the position the reference path draws
it. A draw that no mode can express is not guessed. The instanced path is
refused for views that contain it (section 3.12), and the implementer
reports it to the coordinator for a spec revision. `[Safe rule]`

Revision 2 gave a slot and a mode to the three draws of this kind that the
Wave 3 implementer reported. Revision 3 restates them with the measured
numbering, as one rule (section 3.3 gives each in full): `[Must-match]`

| Draw | Layer | Primary | Secondary | Mode | Light source |
| --- | ---: | ---: | --- | --- | --- |
| Enhanced shroud | 3 | 10000 | Shroud draw rank (0 in play) | `remap`, one constant row | `none` |
| Ground-item highlight k, with n list entries | 4 | 0 | 8 × (n + k) + part | As an outline of its ink: `plain` or `outline_blend` | `none` |
| Player cutout in an unlit scene | The item's: 1, 2 or 3 | The item's | The item's, part 0 | `egg` | `none` |

### 3.3 Gathering Draw Items

Each layer's items are the ones the reference path draws in that layer for
R, with the art frame, position and light the reference path uses (SPEC-001
section 3.5, SPEC-003 section 3.6). Sprite pixels are copied unchanged into
the GPU atlas. `[Must-match]`

- **Floor (layer 0).** Each floor square the reference path draws for R.
  - A **uniform** tile (SPEC-014 section 3.44, "Floor light", step 3) is mode
    `lit`, light source `fixed`, with its common vertex light, and `EXEMPT`
    set.
  - An **interpolated** tile (step 4) is mode `floor_map`, light source
    `level image`. Its level image is the tile's size, and holds, for each
    tile pixel, the light level the reference path uses for that pixel
    (section 3.9). `EXEMPT` is set exactly when the reference path leaves
    indexes 229-255 unlit in interpolated tiles (the step 4
    `[May-improve]`).
  - `[Must-match]`
- **Flat and upright (layers 1 and 2).** Each object of the flat pass and
  of the upright pass (SPEC-014 section 3.44, "Two passes"), for the visited
  hexes the reference path visits for R and the current elevation.
  - With exactly one of the translucency bits `0x4000`, `0x10000`,
    `0x20000`, `0x40000`, `0x80000` set, and not `0x8000`, it is mode
    `translucent` with the blend table of section 2.2. Wall translucency
    uses light source `fixed` with 65536; the others use light source `hex`.
  - Otherwise it is mode `lit`, light source `hex`, with `EXEMPT` set.
  - A wall or scenery object whose cutout applies (SPEC-014 section 3.37)
    is split as section 3.5 says.
  - `[Must-match]`
- **Roof (layer 3).** Each drawn roof square: not hidden, art index not 1
  (SPEC-014 section 3.36, with the hide state from `vault_core::roof`). There
  are none when the enhanced `roof_peek` mode is `hidden` or the debug camera
  removes roofs. Each is mode `lit`, light source `ambient`, with `EXEMPT`
  set, and is split by the cutout as section 3.5 says. `PEEK` is set on
  every roof instance while the reference path applies the cursor X-ray
  (section 3.10). `[Must-match]`
- **Enhanced shroud (layer 3, primary 10000; revision 2).** The reference
  path draws the enhanced view's shroud after the roofs and before the
  outlines (measured by the implementer, Wave 3). Each shroud draw is one
  instance of mode `remap`, light source `none`, clip R, with secondary its
  shroud draw rank. Its image and remap table are recorded by the reference
  path's shroud painter, run with all its clipping, but recording instead of
  writing the canvas: `[Must-match]`
  - At each canvas pixel the draw writes, the painter's result is a function
    of D, the canvas index there just before the draw. The recorder
    tabulates that function at all 256 values of D, as one 256-entry row. A
    pixel the draw leaves alone records 0 in the image.
  - Equal rows share one code. The image pixel is the code c, 1 to 255, and
    the table's row c − 1 holds the function. The numbering of the codes is
    free, because the canvas does not depend on it. `[May-improve]`
  - A result that depends on anything other than D at that pixel (another
    pixel's index, for example), or a draw that needs more than 255 distinct
    rows, is refused (section 3.12). `[Safe rule]`
  - The recorder may merge consecutive shroud draws of one refresh into one
    instance of rank 0, recording at each pixel the composition of the
    functions applied there, if the merged instance still needs at most 255
    rows. `[May-improve]`
  - **The measured rule (revision 3).** The painter writes one constant
    palette index, the **dark index**, over D. It does not read D, and it does
    not darken through the intensity table (measured by the implementer,
    2026-10-30). So the recorder gives one instance covering the whole view,
    whose table has one row holding the dark index 256 times, and whose image
    is 1 where the painter writes and 0 elsewhere.
  - With b the Bayer value of world pixel (wx, wy) (section 3.10), a pixel
    takes the dark index when any of these holds: its hex is off the grid;
    its hex is outside the allowed set and has no allowed center within 104
    world pixels; or its hex is outside the allowed set and `d² ≥ (20 + 4 ×
    b)²`, with d the pixel's distance from the allowed center nearest to that
    hex, ties going to the lowest hex number. A pixel over an allowed hex
    never changes. So the dither band runs from 20 to 80 pixels, and every
    pixel is dark from 80 on (`rev3/shroud-mask.tsv`).
  - The shell's dark index is the mapped entry below index 229 nearest to
    black, and the shell allows the whole grid, so in play only pixels off
    the grid take it. The in-world cursor's art is drawn in the flat pass, so
    it is shrouded with the scene. Outlines, floating text and objects
    without a hex stay above the shroud.
  - The painter stays the authority: the rule above checks the recorder, and
    never replaces it. If they differ, the reference path wins and the
    difference is reported for a revision.
- **Outline (layer 4).** One instance for each entry of the refresh's
  outline list (at most 100), in list order, then one for each ground-item
  highlight, in its order, at the running positions of section 2.1
  (revision 3). A highlight is drawn exactly like an outline of its type. Its
  image is the outline's **ring image**: the reference path's outline painter
  (SPEC-014 section 3.44, "Outline geometry"), run with all its clipping and
  buffer guards, but recording each paint instead of writing the canvas. The
  ring image covers the frame widened by one pixel on every side.
  `[Must-match]`
  - **Which items are highlighted (revision 3).** While Shift is held on the
    map, each drawn object that is an item, on a hex, whose own outline word
    draws nothing, is collected in a second list during the flat and upright
    passes, in draw order. That list has no size limit. The limit of 100
    applies only to the outline list, so an item past the first 100 outlined
    objects is still highlighted, and a highlight never pushes an outline off
    the list (measured by the implementer, 2026-10-30).
  - **What a highlight writes (revision 3).** What an outline of its word
    writes, at each paint of the ring: an opaque ink replaces D with its
    index, a banded ink with the index of the band for the scanned row, and a
    translucent ink with `F_t[g][D]`, so a pixel painted twice blends twice.
    The shell's word is the plain item outline, so in play each paint writes
    the item color's index.
  - For an opaque outline, the ring pixel is the index of the paint that
    stays (the column pass's when there is one), and 0 where nothing was
    painted. The instance is mode `plain`. A paint of index 0 cannot be told
    from "not painted", so it refuses the refresh (section 3.12). `[Safe rule]`
  - For a translucent outline, the ring pixel is the number of paints, 1 or
    more, and 0 where nothing was painted. The instance is mode
    `outline_blend`, with the outline color's blend table and its common
    gray level in aux.
  - The ring is part 0. A right paint that the reference path wraps to
    column 0 of the next canvas row (the `[May-improve]` of SPEC-014 section
    3.44, step 5) lies outside the ring image. All such paints of one
    outline are recorded in a **wrap column**: a one-pixel-wide image at
    canvas column 0, spanning the rows they reach, drawn as part 1 of the
    same outline with the ring's mode. If the reference path skips wrapped
    paints, there is no wrap column.
- **Floating text (layer 5).** Each live text box, in creation order, as the
  image the reference path composes for it (SPEC-014 section 3.46), mode
  `plain`. `[Must-match]`
- **No-hex (layer 6).** Each object without a hex that the reference path
  draws, in its order, with the mode chosen as for layers 1 and 2 but light
  source `fixed` at 65536. `[Must-match]`
- **Unlit scenes (revisions 2 and 3).** An unlit scene is a refresh that the
  reference path draws without a light (no light buffer and no ambient). The
  shell always passes a light, so unlit scenes arise only in tools and tests.
  `[Must-match]`
  - Its cutout still blends, but from the unrelit source pixel (measured by
    the implementer, Wave 3). The `egg` part of section 3.5 uses light source
    `none`, under which the source is not relit (section 3.6). The cutout runs
    at its usual places: in the flat and upright passes for the walls and
    scenery it cuts, and in the roof pass for every roof square.
  - Every other item that the scene would light draws its source pixel s, as
    mode `plain`: floor squares, objects, roofs and the strips of section
    3.5. The translucency bits are ignored too, so a translucent object draws
    s, not a blend (revision 3, measured by the implementer, 2026-10-30).
    Outlines, highlights and floating text do not change.
    `rev3/unlit-modes.tsv` lists every item.

### 3.4 Sort Order

1. **Layers first.** Every item of a lower layer is drawn before every item
   of a higher layer. `[Must-match]`
2. **Flat and upright.** Within layers 1 and 2, ascending hex number, then
   ascending list index, then ascending part. This is SPEC-014 section 3.44's
   order: every flat object before any upright one, and within each pass by
   ascending hex number and list order. No screen-y, sprite-bottom,
   multihex, art or identifier tie-break is added. `[Must-match]`
3. **Reinsertion.** An object's list index changes exactly when SPEC-014
   section 3.44 reinserts it: a move to a hex, a pixel-offset change, every
   animation frame step (SPEC-007 sections 3.10 and 3.11), and a flat flip.
   The key is computed from the list as it stands when the refresh gathers.
   `[Must-match]`
4. **Squares.** The square rank of a floor or roof square is its position in
   the order of **all 10000 squares**, sorted by ascending **corner y**, then
   ascending **corner x**, then ascending square index `column + 100 × row`
   (measured by the implementer, Wave 3 and 2026-10-30; Decision D-10). The
   corner is the square's bounding-image corner of SPEC-003 section 3.6: the
   floor corner for a floor square, the roof corner for a roof square. Each
   corner is the grid origin plus a fixed offset, so the rank of a square is
   the same in every view, and a square has one rank on the floor and on the
   roof. `[Must-match]`
   - **Skipped squares (revision 3).** The rank is a position among all
     squares, so squares that are not drawn leave gaps, as they do in the
     reference path's draw log. The draw skips a square whose corner lies past
     the view's right or bottom edge, a hidden square, a square without art,
     and a roof of index 1. Revision 2 counted only the drawn squares from 0,
     which gave the same order but other numbers (D-34).
   - Revision 1 recorded ascending square index instead. The two orders
     differ: in a row, a step of one column moves the corner 12 pixels down,
     and a step of one row moves it 24 pixels down, so square
     `column + 100 × (row + 1)` comes before square `column + 3 + 100 × row`
     (section 5.5).
   - Within one layer two distinct squares never share a corner (the
     projection is one-to-one), so the index never decides in practice. It
     is kept as the stated last key.
   - The roof corner is the floor corner moved 96 pixels up, so floor and
     roof squares of the same set have the same ranks.
5. **Shroud, outlines, highlights, texts, no-hex objects** follow their
   shroud draw, list, highlight, creation and reference orders (section 2.1).
   `[Must-match]`
6. **Uniqueness.** Two instances with the same key, or an item whose field
   is outside section 2.1's domain, refuse the refresh (section 3.12).
   `[Safe rule]`
7. **Stability.** The order depends only on the keys. It does not depend on
   hash-map iteration, thread scheduling, upload order or GPU timing.
   `[Must-match]`

**Order parity.** For every refresh, the sequence of draws the reference path
makes (one per object, square, outline, text box, or cutout part) equals the
sorted instance list, item for item. Section 4 test T9 checks this with a
draw-recording hook in the reference path. `[Must-match]`

### 3.5 Splitting by the Player Cutout

When the cutout applies to an object or roof square (SPEC-014 section 3.37),
and the mask rectangle M overlaps the item's rectangle clipped to R (call it
C), the item becomes up to five instances. Every part keeps the item's layer,
primary and list index. `[Must-match]`

| Part | Rectangle | Mode |
| ---: | --- | --- |
| 0 | C ∩ M | `egg`, with the item's light source |
| 1 | The rows of C above M, full width of C | The item's normal mode |
| 2 | The rows of C below M, full width of C | The item's normal mode |
| 3 | The rows of C ∩ M, columns of C left of M | The item's normal mode |
| 4 | The rows of C ∩ M, columns of C right of M | The item's normal mode |

An empty part is not emitted. Without an overlap the item is one part-0
instance in its normal mode. Each part's src x and src y select the matching
pixels of the item's frame. The `egg` part never sets `EXEMPT`: the reference
path has no exemption for indexes 229 and up inside the mask (revision 3,
measured by the implementer, 2026-10-30). When the mask art is missing, or the
player has no hex, the instanced path follows the reference path's choice
for SPEC-014 section 3.37 "Failures". `[Must-match]`

### 3.6 Draw-Mode Semantics

An instance draws its rectangle pixel by pixel. For instance pixel (u, v),
with 0 ≤ u < width and 0 ≤ v < height: `[Must-match]`

1. The canvas pixel is `(x, y) = (dst x + u, dst y + v)`. If (x, y) is
   outside the clip or outside the canvas, nothing happens.
2. The source value is `s = image[src y + v][src x + u]`. If s is 0, nothing
   happens: index 0 is transparent in every mode (SPEC-001 section 3.1), and
   a ring pixel 0 means "not painted".
3. If `PEEK` is set, the uniform block's peek bit is set, and the peek test
   of section 3.10 cuts (x, y), nothing happens.
4. Otherwise D is the canvas index at (x, y), read just before the write, and
   the pixel becomes the mode's result.

**Light level.** For the light sources: `[Must-match]`

- `hex`: E = max(ambient, B[hex]), where B is the hex light buffer. A hex
  outside 0 to 39999 gives B = 0, so E = ambient.
- `ambient`: E = ambient.
- `fixed`: E = clamp(fixed light, 0, 65536).
- `none`: no level. Only modes 0, 5 and 6, and mode 4 in an unlit scene
  (revision 2), use it.

The level is `L = clamp(E, 0, 65536) >> 9`, from 0 to 128 (SPEC-014's light
level divided by 512). A `level image` pixel gives L directly.

**Results.** Let `lit(s, L)` be s when the light source is `none` (revision
2), s when `EXEMPT` is set and s ≥ 229, and `I[s][L]` otherwise. With F_t
the blend table t, `F_t[g][D]` its entry at `256 × g + D` (SPEC-002 section
3.2), and `gray(s)` the glass gray level of s for table 1 and the common gray
level otherwise (SPEC-014 section 3.44), clamped as below: `[Must-match]`

| Mode | Result |
| --- | --- |
| 0 `plain` | s |
| 1 `lit` | lit(s, L) |
| 2 `floor_map` | lit(s, L), with L the level image's pixel `level[level y + v][level x + u]` |
| 3 `translucent` | `I[F_t[gray(s)][D]][L]`, t = aux bits 0-2. No exemption for 229-255 |
| 4 `egg` | Let s1 = lit(s, L) and m the cutout mask's value at `(x - cutout left, y - cutout top)`, or 0 outside the mask. If m is 0 the result is s1, otherwise `X[I[s1][m]][I[D][128 - m]]` |
| 5 `outline_blend` | Start with r = D, then repeat s times: r = `F_t[g][r]`, with t = aux bits 0-2 and g = aux bits 8-10. The result is r. No light is applied |
| 6 `remap` | `M[s − 1][D]`, with M the instance's remap table (revision 2). No light is applied |

A mask value above 128 is used as 128. `[Safe rule]`

A `remap` source value above the table's row count refuses the refresh; the
recorder never produces one. `[Safe rule]`

**Gray-level bound (revision 2).** A blend table has 8 rows, g = 0 to 7.
The gray level that selects the row, `gray(s)` in mode 3 and the outline
color's common gray level that goes into aux for mode 5, is clamped to
`min(gray, 7)` before it is used, on the reference and instanced paths
alike. `[May-improve]` (an out-of-bounds row index replaced by a stated
bound; section 5.7 and `rev2/blend-clamp.tsv`).

- **Parity is unaffected.** Palette gray levels are computed from five-bit
  components (each six-bit channel halved first, SPEC-014 section 3.44), so
  even the brightest entry, (63, 63, 63), gives `trunc(trunc(310 / 10) / 4)
  = 7` in both gray tables. No parsed palette and no palette-built outline
  color has a gray level above 7, so the clamp changes no output that a
  palette can produce (measured by the implementer, Wave 3 follow-up).
- **What it guards.** Only a hand-built color, such as a translucent outline
  ink constructed in code with a gray level of 8 or more, could index past a
  table's 8 rows. The reference path now clamps it (in the translucent draw
  and in translucent outlines) instead of panicking, and the instanced path
  applies the same clamp, so the two paths stay equal for such inputs too.

Mode 5 applies the blend once for each paint, so a ring pixel painted twice
blends a second time over its own first result, as SPEC-014 section 3.44
says. `[Must-match]`

### 3.7 Batching

**Kinds.** Modes 0, 1 and 2 are **overwrite** modes. Modes 3, 4, 5 and 6 are
**read** modes: they read D. `[Must-match]`

**Planning.** Walk the sorted, culled instances in order, and keep the
current batch. Let r be the instance's rectangle clipped to its clip. The
instance joins the current batch when all of these hold; otherwise it starts
a new batch: `[Must-match]`

1. its kind is the batch's kind;
2. its clip equals the batch's clip;
3. for a read batch: r is disjoint from the clipped rectangle of every
   instance already in the batch.

So an overwrite batch is a maximal run of overwrite instances with one clip,
and a read batch is a run of read instances with one clip whose clipped
rectangles do not overlap one another.

**Execution.** `[Must-match]`

- An **overwrite batch** is one instanced draw call, with the scissor set to
  its clip, drawing its instances in key order. Where two instances of the
  batch cover the same pixel, the later one's write is the one that stays.
  This relies on the primitive-order guarantee of the graphics APIs that wgpu
  targets (Vulkan, Metal, Direct3D 12, OpenGL): within one draw call, color
  writes to one pixel land in submission order.
- A **read batch** first copies a **snapshot** of the canvas, covering at
  least the bounding box of its instances' clipped rectangles, into a
  second `R8Uint` texture. It then makes one instanced draw call, in which
  every instance reads D from the snapshot. Because the rectangles are
  disjoint, this equals drawing the instances one at a time.
- A batch may be split into consecutive draws (for a buffer size limit, for
  example), but batches are never merged, reordered, or split across a read
  boundary. `[Safe rule]`

The snapshot may cover the whole canvas, or only the bounding box.
`[May-improve]`

**Draw count.** The number of draw calls of a refresh is its number of
batches, and the number of snapshot copies is its number of read batches. A
refresh whose instances all use overwrite modes and one clip is drawn with
exactly one draw call. `[Must-match]`

The engine exposes these counts per refresh (instances, batches, read
batches) for tests and for the F9 debug overlay. `[May-improve]`

### 3.8 The Hex Light Buffer

1. **Source.** The local light of hex h on the current elevation is the
   world's accumulated light of SPEC-006 section 3.6: 655, plus the
   contribution of every enabled, visible light source under its blocking
   rules. A source of intensity V and radius r at hex distance D ≤ r adds
   `V - D × trunc((V - 655) / (r + 1))`, and its own hex receives V. The
   renderer reads these values from the world; it does not compute light
   itself. `[Must-match]`
2. **Clamp.** Entry h is `clamp(local light, 0, 65536)`. The upper bound is
   SPEC-014 section 3.44's cap at 65536. The lower bound, 0, guards against a
   negative accumulation (for example from a light of negative intensity).
   `[Must-match]` for the upper bound, `[Safe rule]` for the lower.
3. **Ambient.** The ambient light is not added into the buffer. The shader
   combines the two by SPEC-006 and SPEC-014's maximum rule, E = max(ambient,
   B[h]) (section 3.6, Decision D-11). The ambient value is the world's
   current ambient light (16384 to 65536 after SPEC-006's clamp), clamped
   again to 0 to 65536 when it is written into the uniform block.
   `[Must-match]`
4. **Freshness.** Before a refresh is drawn, the buffer holds the light of the
   current elevation as the reference path would read it for that refresh: it
   is uploaded after a map load, an elevation change, any light rebuild, and
   any change of an object's light. `[Must-match]` The implementation may
   upload only the changed rows, or keep one buffer per elevation.
   `[May-improve]`
5. **Fixture digest.** For a world with no light source, every entry is 655
   and the digest of the byte image is `DC22FAE74E4BBAA5`. A buffer of 40000
   zeros has digest `5D221D5070447725`. `[Must-match]`

### 3.9 Floor Light

1. The level image of an interpolated floor tile holds, for tile pixel (px,
   py), `trunc(entry / 512)`, where entry is the light-map entry the
   reference path reads for that pixel (SPEC-014 section 3.44, "Floor light",
   step 4: entry `160 + 80 × py + px`). `[Must-match]`
2. The values are taken from the reference path's own floor-light routine
   (`vault_render::map::render_floor_lit`, as `docs/REFERENCE.md` names it),
   refactored to return the levels of a tile instead of drawing it, and run
   in the reference path's tile order. Whatever the reference path does with
   the vertex-0 rule for odd base hexes and with entries that no triangle
   writes (the two `[May-improve]` items of steps 2 and 5), the level image
   holds the same values, including values left over from the tile drawn
   before. `[Must-match]`
3. A level above 128 cannot come from the interpolation of values capped at
   65536. A level image that holds one refuses the refresh. `[Safe rule]`
4. The level images may be computed on the GPU, for example by a compute pass
   over the hex light buffer, only if every value equals the CPU routine's.
   `[May-improve]`
5. Floor and roof instances, with their level images, may be cached across
   refreshes, and rebuilt only when the view, the elevation, the tile words,
   the roof hide state, the light buffer or the ambient light changes.
   `[May-improve]`

### 3.10 The Roof Peek Mask

The enhanced view's cursor X-ray (`docs/REFERENCE.md`, "Roof peek") cuts the
roofs away in a disc around the cursor, and dissolves them in a ring around
the disc with a 4-by-4 ordered dither anchored to world pixels. Its size is
constant in interface pixels at every zoom. The parameters are computed on
the CPU, in `vault_render::map::enhanced`, and passed in the uniform block.

Revision 2 states the reference path's own rule, measured by the implementer
in Wave 3. Revision 1's reading of the documentation (a 1/16-pixel center at
the pixel middle, and thresholds at `(t + ½)` ring steps) differed from the
reference by 91 to 550 pixels per case on the showcase map, at 3 cursor
positions and zooms 1.0, 1.25 and 2.0. It is withdrawn.

**Parameters.** `[Must-match]`

- **Zoom.** `zoom` is the enhanced view's zoom, in interface pixels per
  canvas pixel. It leaves out the UI scale: the radii are 96 and 48
  interface pixels, whatever the window's scale factor.
- **Radii.** In whole canvas pixels:
  - inner radius `inner = round(96 / zoom)`;
  - ring width `ring = round(48 / zoom)`;
  - outer radius `outer = inner + ring`.

  `round` rounds half away from zero (Rust's `f64::round`). The division is
  done in double precision (binary64), and a zoom held in binary32 is widened
  first (revision 3, measured by the implementer, 2026-10-30). Every zoom of
  the fixtures is exact in binary32 and binary64, and none of its quotients
  lies within 0.001 of a half-integer.
- **Center.** The canvas pixel `(cx, cy)` under the cursor:
  `(floor(px), floor(py))` for the cursor's position (px, py) in canvas
  coordinates, pixel (x, y) spanning x to x + 1. Distances are measured from
  that pixel's corner to the tested pixel's corner, so they are whole
  numbers, and the disc never moves by less than a pixel.
- **Tiles (revision 3).** Each tile of the enhanced canvas is its own view
  (section 3.2), and takes the same center less the tile's corner: a tile
  with corner (tx, ty) uses `(floor(px) - tx, floor(py) - ty)`. The grid
  origin moves with the tile, so a world pixel has the same b and d in every
  tile that draws it (`rev3/peek-center.tsv`, `rev3/peek-seam.tsv`).
- **Thresholds.** For b = 0 to 15, `Q[b] = (16 × inner + (b + 1) × ring)²`.
  The uniform block holds `Q[b]` (section 2.4).

**Safe bounds.** `[Safe rule]`

- If `outer` would exceed 1024, inner becomes `min(inner, 1024)` and ring
  becomes `min(ring, 1024 - inner)`. Then every `Q[b]` is at most `2^28`.
- The center is clamped to `-2^20` to `2^20` in each axis.
- A zoom that is not finite, or not above 0, gives no parameters: the
  instanced path refuses a refresh with the peek active (section 3.12), so
  the reference path draws it.
- **These bounds are the instanced path's own (revision 3).** The reference
  path has no radius cap and no center clamp (measured by the implementer,
  2026-10-30). The shell's zoom range of 0.5 to 3.0 never reaches the cap,
  because outer is at most 288 there. So when a cap or a clamp changes a
  value, the parameters are still computed as above, but the instanced path
  refuses the refresh, and the reference path draws it (section 3.12, D-38).
  A pixel is never drawn from a capped radius.

**Dither matrix.** The standard 4-by-4 Bayer matrix, row by row (unchanged
from revision 1): `[Must-match]`

```text
 0  8  2 10
12  4 14  6
 3 11  1  9
15  7 13  5
```

**Peek test** for canvas pixel (x, y): `[Must-match]`

1. World pixel `(wx, wy) = (x - OX, y - OY)`: the canvas pixel minus the
   grid origin, so the pattern moves with the world, not with the canvas.
   `b = Bayer[wy mod 4][wx mod 4]`, with the non-negative remainder.
2. `dx = x - cx`, `dy = y - cy`.
3. If `|dx| > outer` or `|dy| > outer`, the pixel is kept.
4. `d² = dx² + dy²`. If `d² ≤ inner²`, the pixel is cut. The inner disc
   includes its boundary.
5. If `d² > outer²`, the pixel is kept.
6. Otherwise the pixel is in the ring. It is kept when
   `256 × d² ≥ Q[b]`, and cut when `256 × d² < Q[b]`.

So the pixels at distance `inner` or less are always cut, the pixels beyond
`outer` are always kept, and in the ring a pixel of Bayer value b survives
from distance `inner + (b + 1) × ring / 16` outward. A pixel at exactly
`outer` is kept for every b (with b = 15, `Q[15] = 256 × outer²`). Steps 3 and
5 are early exits: step 6 alone gives the same answer beyond `outer`.

With `t = (d - inner) / ring`, step 6 says that a ring pixel is kept exactly
when `b < floor(16 × t)` (`rev3/peek-ring.tsv`). The test in `d²` is the rule,
because it needs no square root. A cut pixel leaves what lies beneath it. A
kept pixel draws as it would without the X-ray: through the player cutout
inside the mask, otherwise the lit draw. Only roofs are cut. `[Must-match]`

Every value fits in an i32. A canvas is at most 16384 pixels wide (section
3.11), so with the center's clamp |dx| and |dy| stay below 2^21. After step
3 they are at most 1024, so `256 × d²` is at most 2^29.

**Parity.** This is the reference path's X-ray, which defines the gallery's
`roof-xray-peek.gif` (D-4). Test T8 compares the two paths on the showcase
map. If a later measurement shows a difference, the implementer changes
neither path's peek: the instanced path refuses refreshes with the peek
active, so the reference path draws them (section 3.12), and the difference
is reported for a spec revision. `[Safe rule]`

### 3.11 GPU Requirements

1. **Integer only.** The canvas is an `R8Uint` texture. Sprites, ring
   images, level images, shroud images and remap tables, the cutout mask,
   the gray tables, and the
   intensity, mix and blend tables are `R8Uint` textures. The hex light
   buffer is `R32Uint`. Every read uses `textureLoad` at integer
   coordinates. No sampler, filtering, normalized format, sRGB conversion,
   blend state, multisampling or floating-point color math is used.
   `[Must-match]`
2. **Tables.** The lookup tables are the ones the reference path uses at that
   moment, from the same palette load (SPEC-001 section 3.7, through
   `vault_formats::palette`, and SPEC-002 section 3.2). They are uploaded again
   whenever the reference path would rebuild them: each palette file load
   for the intensity, mix and blend tables. Palette cycling and fades change
   only the display palette, which the canvas does not use. `[Must-match]`
3. **Coverage.** A fragment's pixel is `floor` of its position. The fragment
   shader discards a pixel outside the instance's rectangle, so coverage is
   exact whatever the quad's floating-point edges. The quad may be expanded
   by one pixel on every side to make sure that every pixel of the
   rectangle is reached. `[Must-match]` for the coverage, `[May-improve]`
   for the expansion.
4. **Write.** A fragment either discards or writes one index, with the
   color write mask set for the channel. No depth or stencil attachment is
   needed. `[May-improve]`: an implementation may use a depth test to
   reorder overwrite instances, only if every canvas byte stays the same.
5. **Atlas.** Sprite frames are cached in atlas pages (an `R8Uint` texture
   array), keyed by art identifier (SPEC-007 section 2.1), direction and
   frame. The page layout and the eviction rule are free, under the
   reference path's 256 MiB art-cache limit. `[May-improve]`
6. **Limits.** A canvas wider or taller than 16384 pixels, or a canvas, page
   or level image larger than the device's texture limits, refuses the
   refresh. `[Safe rule]`

### 3.12 Fallback and Errors

1. **No device.** When no wgpu adapter or device can be created, or the
   device is lost, the session uses the reference path to its end. This is
   logged once, and the setting is not changed. `[Safe rule]`
2. **Refused refresh.** A refresh is refused for an unexpected state: a
   layer 7 item, a field outside its domain, two equal keys, an unknown mode
   or light source, a level above 128, a texture limit, a full atlas, an
   upload error, a draw no mode can express (section 3.2), a shroud draw
   the `remap` recorder cannot express or a remap source value above its
   table (section 3.3), an opaque ring paint of index 0 (section 3.3), a peek
   zoom that is not finite and positive, a peek cap or clamp that changed a
   value, or the peek mismatch of section 3.10. The refusal is a typed error.
   That refresh is
   then drawn by the reference path as a refresh of the **whole** canvas,
   and the result replaces the GPU canvas. Each cause is logged once per
   session. `[Safe rule]`
3. **No panic.** No input makes the instanced path panic, read outside a
   buffer or texture, or loop without bound. A ring pixel's paint count is
   at most 255, so mode 5 repeats at most 255 times. `[Safe rule]`

A refused refresh redraws the whole canvas, not just R. That can remove a
stray outline pixel that an earlier refresh left outside its rectangle, a
difference that only follows a refusal. `[Safe rule]`

### 3.13 What the Path Never Changes

1. Drawing never changes the world, the RNG, the timed queue or any script
   state. SPEC-021's `state_hash` is the same before and after a refresh on
   either path. `[Must-match]`
2. The `render_path` setting writes no input-log record, changes no replay
   record or digest, and is not saved. A replay's frame digest is the same on
   both paths, because the canvases are equal. `[Must-match]`
3. Picking, blocking, roof hiding and the outline word are computed by the
   world and the reference rules, never from GPU output. `[Must-match]`
4. `cargo xtask capture-gallery && git diff --exit-code docs/assets` passes
   on the default path. No file under `docs/assets` is re-recorded for this
   spec. `[Must-match]`

### 3.14 Module Placement

`[May-improve]` unless tagged.

- The layer table, sort key, instance, batch planner and a CPU instance
  executor (section 4, T5) belong in `vault_render::layers`. The GPU device,
  pipelines, textures and uploads are in a submodule of it, or a sibling
  module, as the implementer chooses.
- The roof peek parameters (section 3.10) are computed in
  `vault_render::map::enhanced`, where the enhanced view's roof peek lives.
- The hex light buffer is packed from the world's light by the map renderer
  (`vault_render::map`). The roof hide state comes from `vault_core::roof`.
- The interface names `RenderPath`, `FrameLayer`, `SortKey`, `SpriteInstance`,
  `DrawMode`, `LightSource`, `BatchPlan`, `HexLightBuffer`, `LayerUniforms`
  and `RoofPeekParams` are Team A's (Decision D-21). An implementer may rename
  them, and records the mapping in the commit.
- The shroud recorder (section 3.3) belongs beside the reference path's
  shroud painter in `vault_render::map::enhanced`, as the outline recorder
  belongs beside the outline painter. The new mode is `DrawMode::Remap` in
  Team A's naming.

### 3.15 Revision 2 Files, Supersession, and the Implementer's Follow-Up

Revision 2 adds the folder `tests/fixtures/spec022/rev2/`. The revision 1
files stay where they are, because the merged tests read them, until the
implementer's follow-up adopts revision 2. `[Must-match]`

| File | Status in revision 2 |
| --- | --- |
| `peek-scale.tsv`, `peek-thresholds.tsv`, `peek-samples.tsv` | **Superseded** by the files of the same names in `rev2/` (the revision 2 peek rule) |
| `scene-uniforms.hex` | **Superseded** by `rev2/scene-uniforms.hex` (offsets `0x30` to `0x7F` differ) |
| `scene-expected.hex` | **Superseded** by `rev2/scene-expected.hex` (two roof pixels differ, at (1, 0) and (0, 1)) |
| `sort-keys.tsv` | **Superseded** by `rev2/sort-keys.tsv`, which keeps every revision 1 row (floor and roof rows relabelled as ranks) and adds five |
| `scene.tsv`, `scene-order.txt`, `scene-batches.tsv`, `scene-sprites.txt` | Unchanged; the revision 2 scene uses them with the new peek |
| `light-falloff.tsv`, `light-pack.tsv`, `light-levels.tsv` | Unchanged |
| `rev2/square-order.tsv` | New: the square ranks of section 5.5 |
| `rev2/scene2.tsv`, `rev2/scene2-sprites.txt`, `rev2/scene2-order.txt`, `rev2/scene2-batches.tsv`, `rev2/scene2-uniforms.hex`, `rev2/scene2-expected.hex` | New: the second scene of section 5.6 |
| `rev2/blend-clamp.tsv` | New: the gray-level bound of section 5.7 |
| `rev2/README.md` | New: describes the folder |

**The implementer's follow-up** (Team B or the coordinator):

1. Switch T1, T4, T5 and T8 to the `rev2/` files, add T15 to T17, and extend
   T5 to the second scene.
2. Change the reference path's peek only if T8 then shows a difference from
   the revision 2 rule. Revision 2 describes the reference path as measured,
   so the expected change is on the instanced side and in the parameter
   math.
3. Implement the shroud recorder, the `remap` mode, the highlight positions,
   the `none` light source of the `egg` mode, and the gray clamp on the
   instanced path (the reference path already clamps), and remove the
   corresponding refusals, including the instanced path's refusal of a gray
   level above 7.
4. Delete the superseded revision 1 files listed above in the same commit,
   and point `tests/fixtures/spec022/README.md` to `rev2/`.
5. `cargo xtask capture-gallery && git diff --exit-code docs/assets` still
   passes on the default path. The gray clamp changes no palette-derived
   output, so no asset is re-recorded for it.

Revision 3 carries this list on in section 3.16. A step above that is already
done stands.

### 3.16 Revision 3 Files, Supersession, and the Implementer's Follow-Up

Revision 3 adds the folder `tests/fixtures/spec022/rev3/`. Team A changed no
file of revisions 1 or 2, because the merged tests may still read them, and
those files stay until the follow-up below adopts revision 3. `[Must-match]`

| File | Status in revision 3 |
| --- | --- |
| `rev2/square-order.tsv` | **Superseded** by `rev3/square-rank.tsv`: same order, with the rank among all 10000 squares |
| `rev2/scene2.tsv`, `rev2/scene2-order.txt` | **Superseded** by the files of the same names in `rev3/`: only `hl`'s key and secondary differ |
| `rev2/scene2-batches.tsv`, `rev2/scene2-sprites.txt`, `rev2/scene2-uniforms.hex`, `rev2/scene2-expected.hex` | Current: the numbering changes no batch, byte or pixel |
| `rev2/sort-keys.tsv` | Current. Its highlight rows are the keys of a full outline list |
| `rev2/peek-scale.tsv`, `rev2/peek-thresholds.tsv`, `rev2/peek-samples.tsv`, `rev2/blend-clamp.tsv`, and the rest of `rev2/` | Current |
| `rev3/highlight-positions.tsv` | New: highlight keys for lists of any length |
| `rev3/peek-center.tsv`, `rev3/peek-ring.tsv`, `rev3/peek-seam.tsv` | New: the center, the ring boundary and the tile seam |
| `rev3/shroud-mask.tsv` | New: the shroud's pixel test |
| `rev3/unlit-modes.tsv` | New: each item's mode in an unlit scene |
| `rev3/README.md` | New: describes the folder |

**The four refusals.** These are the refresh states the instanced path refuses
today, as `docs/REFERENCE.md` lists them, each with the rule that lifts it and
the test that proves it:

| # | Refused today | Lifted by | Proved by |
| ---: | --- | --- | --- |
| 1 | The cursor roof peek: any refresh with the X-ray active | Section 3.10 | T8 |
| 2 | The enhanced shroud past the grid, so every unlocked widescreen refresh falls back | Section 3.3, layer 3 primary 10000, `remap` | T17 (a), T18, T10, T11 |
| 3 | A ground-item highlight whose running number is 100 or more | Sections 2.1 and 3.3, position `n + k` | T1, T17 (b) |
| 4 | A cutout drawn without a light | Section 3.3, `egg` with light source `none`, and `plain` for the rest | T17 (c) |

**What the implementer changes.** One follow-up commit by the implementer (not
Team A) does all of this. Refusals stay in place until the step that lifts
them, so the canvas is right at every point. `[Must-match]`

1. **Numbering.** Give the instanced path's floor and roof keys the rank among
   all 10000 squares. Then the two numbering differences are gone: the keys
   and the reference path's draw log give the same numbers. The highlights
   already use the running number. T9 compares the numbers themselves, not
   only the order, and T15 reads `rev3/square-rank.tsv`.
2. **Refusal 1, the peek.** Compute the parameters of section 3.10 (whole
   pixels, double precision, the center less the tile's corner), write them to
   the uniform block, set `PEEK` on every roof instance, and remove the
   refusal. Refuse only as section 3.12 says, including a cap or clamp that
   changed a value. T8 compares against the revision 2 and 3 rule, and no
   longer against revision 1's.
3. **Refusal 2, the shroud.** Add the shroud recorder (section 3.3) beside the
   painter in `vault_render::map::enhanced`, add the `remap` mode, and draw the
   shroud as one instance at layer 3, primary 10000. Remove the refusal. An
   unlocked widescreen refresh then stays on the instanced path.
4. **Refusal 3, highlights.** Draw a highlight at any running number. Remove
   the refusal from 100 on, and keep the domain of section 2.1.
5. **Refusal 4, the unlit cutout.** Draw `egg` with light source `none`, and
   every other item of an unlit scene as `plain`, with the translucency bits
   ignored. Remove the refusal.
6. **The gray level.** Section 3.15 step 3 asks for this, and it is not one of
   the four: clamp the gray level to 0..=7 on the instanced path (section 3.6),
   and remove its refusal of a level above 7. `[May-improve]`
7. **Tests.** Switch T5 to `rev3/scene2.tsv` and `rev3/scene2-order.txt`, and
   add T18 and T19. T10 and T11 cover the first tile and one other tile of an
   unlocked widescreen view, with the peek active, the shroud on screen and a
   highlight drawn.
8. **Files.** Delete the superseded files of the table above, and those of
   section 3.15 that remain, in the same commit, and point
   `tests/fixtures/spec022/README.md` to `rev3/`.
9. **Gallery and documents.** `cargo xtask capture-gallery && git diff
   --exit-code docs/assets` still passes on the default path. Nothing here
   changes the reference path, so no asset is re-recorded. With a GPU adapter,
   `cargo xtask capture-gallery --render-path instanced` also leaves
   `docs/assets` unchanged, and the pull request records both runs, or says
   that no adapter was available for the second. Update "Refused today" in
   `docs/REFERENCE.md`.
10. **The default stays.** `reference` stays the default render path (D-7).
    Lifting the refusals does not change that: making `instanced` the default
    remains the owner's decision, once T11 is recorded on real hardware and
    the owner's corpus.

If T8, T10 or T11 shows a difference, the implementer changes neither path's
behavior to make the test pass. The reference path wins, the refusal stays for
that state, and the difference is reported for a revision (sections 3.2 and
3.10). `[Safe rule]`

## 4. Required Tests

The implementer writes at least these tests. They use synthetic data only.
GPU tests need a wgpu adapter. Without one they print `skipped: no GPU
adapter` and pass, unless the environment variable `VAULT_REQUIRE_GPU` is
`1`, in which case they fail. Every other test runs everywhere.
`[Must-match]`

- **T1. Sort keys.** The key function gives every row of `sort-keys.tsv`
  (revision 2: `rev2/sort-keys.tsv`), and every row of
  `rev3/highlight-positions.tsv`. Layer 7, a hex of 40000, a floor rank
  of 10000, a roof rank of 10000 given as a roof square (not as the shroud),
  a part of 5 in layers 1 to 3, a part of 1 in layer 0, a part of 2 in layer
  4, a list index of 2^29 and an outline position of 2^29 are each refused
  with an error.
- **T2. Scene order.** Reading `scene.tsv` (written out of order) and sorting
  by key gives the order of `scene-order.txt`. Each row's key equals the key
  computed from its layer, primary and secondary.
- **T3. Batch plan.** Culling and planning the scene gives
  `scene-batches.tsv`: `flat_x` is culled, and there are 8 batches, 3 of them
  read batches. The snapshot column gives the bounding boxes.
- **T4. Uniform block.** Packing the scene parameters (section 5.4) gives the
  128 bytes of `rev2/scene-uniforms.hex`, and those of section 5.6 give
  `rev2/scene2-uniforms.hex`.
- **T5. CPU executor.** A CPU executor implements sections 3.6 and 3.7
  exactly: it runs batches, and read batches through a snapshot. Running the
  scene on a 24-by-16 canvas filled with index `0x40` gives
  `rev2/scene-expected.hex`. A second run that draws the instances one at a
  time, without batches, gives the same canvas. Revision 2: the second scene
  of section 5.6 gives `rev2/scene2-order.txt`, `rev2/scene2-batches.tsv`
  and `rev2/scene2-expected.hex` in the same two ways. Revision 3:
  `rev3/scene2.tsv` gives `rev3/scene2-order.txt`, with the same batches and
  the same canvas.
- **T6. GPU executor.** The GPU path draws the same scene to the same canvas
  (GPU test).
- **T7. Light buffer.** Packing gives every row of `light-pack.tsv`. For the
  falloff world of section 5.3, every row of `light-falloff.tsv` holds,
  every other entry is 655, and the byte image's digest is
  `D45003EF0BA78613`. The no-light and zero digests of section 3.8 hold.
  `light-levels.tsv` gives E and L for each light source. A world built
  through the engine's API with the two light sources of section 5.3 (on
  items with "does not block" `0x10` and "light passes through"
  `0x20000000`, on an otherwise empty elevation) gives the same buffer. If
  it does not, the implementer stops and reports the difference instead of
  changing SPEC-006 behavior.
- **T8. Roof peek (revision 3).** T8 compares against the rule of section
  3.10, never against revision 1's.
  1. The parameter math gives `rev2/peek-scale.tsv` and
     `rev2/peek-thresholds.tsv`, and the center gives
     `rev3/peek-center.tsv`.
  2. The peek test gives every row of `rev2/peek-samples.tsv`,
     `rev3/peek-ring.tsv` and `rev3/peek-seam.tsv`.
  3. A zoom of 0, of -1, and a NaN zoom give no parameters. A zoom of 0.0625,
     which the cap changes, gives the row of `rev2/peek-scale.tsv` and refuses
     the refresh, and the reference path draws it.
  4. Compare the reference path's X-ray with the instanced path's on the
     gallery's showcase map, roof peek `cursor_xray` held, at zooms 0.5, 1.0,
     1.25, 2.0 and 3.0. Use 3 cursor positions, one of them away from a
     pixel's middle (so a whole-pixel center is tested), and at least one
     position whose ring crosses a tile seam of the enhanced canvas. The two
     canvases are equal byte for byte, and the instanced path refuses nothing.
     If they differ, apply the section 3.10 parity rule.
- **T9. Order parity.** A draw-recording hook in the reference path logs
  each draw as (layer, hex or square, list index, part). For the synthetic
  maps (`maps/synthetic.map` from `cargo xtask fixtures --output DIR`,
  `maps/cam_demo.map` in `DIR-cam-demo`, and the gallery showcase map), the
  log equals the instanced path's sorted instance list, numbers included:
  since revision 3 the square ranks and outline positions are the same values
  on both sides, and no test maps one to the other. Cover at least: a
  walking critter (reinsertion on each frame step), two objects on one hex,
  a flat object, the player under a roof (cutout split), a hidden roof
  group, an outline, a floating text, and an object given each translucency
  bit through the engine's API.
- **T10. Canvas parity (CPU).** For every frame of T9's scenarios, the
  reference canvas equals the canvas of T5's executor run on the instanced
  path's instances. Every refresh is a whole-view refresh (section 3.2), so
  revision 1's partial refresh is covered by the enhanced view's canvas
  tiles: cover the first tile and at least one other tile of an enhanced
  widescreen view, at zoom 1.0 and 2.0, with the enhanced shroud and a
  ground-item highlight on screen, and again with the roof peek active
  (revision 3). A sprite that crosses the seam draws in both tiles.
- **T11. Canvas parity (GPU).** The same frames on the GPU path give the
  same canvases (GPU test).
- **T12. Fallback.** Forcing each refusal cause of section 3.12 (a duplicate
  key, an oversized canvas, a lost device) draws the reference canvas and
  logs the cause once. Nothing panics.
- **T13. No state change.** `state_hash` (SPEC-021) is the same before and
  after 100 refreshes on each path. A replay of a synthetic input log on
  both paths gives the same digests.
- **T14. Gallery.** `cargo xtask capture-gallery && git diff --exit-code
  docs/assets` passes on the default path. With a GPU adapter,
  `cargo xtask capture-gallery --render-path instanced` also leaves
  `docs/assets` unchanged. The pull request records both runs, or says that
  no adapter was available for the second.
- **T15. Square order (revisions 2 and 3).** Ranking all 10000 squares by
  section 3.4 item 4, from their floor corners and again from their roof
  corners, gives the `rank` column of `rev3/square-rank.tsv`. The ranks do not
  change with the grid origin. T9's log of a synthetic map shows the reference
  path's floor and roof squares with those ranks, gaps included.
- **T16. Gray bound (revision 2).** With the synthetic gray rule of section
  5.7, every row of `rev2/blend-clamp.tsv` holds on both paths. A
  hand-built outline ink with gray level 8, and one with 15, draws with row
  7 on both paths, without a panic or a refusal.
- **T17. Unslotted draws (revisions 2 and 3).** On a synthetic map, each of
  these is drawn by the instanced path, not refused, and matches the reference
  canvas:
  - (a) an unlocked widescreen view with the enhanced shroud past the grid;
  - (b) a view whose outline list is full (100 entries) with at least two
    ground-item highlights, so that their running numbers are 100 and 101; and
    a view whose list holds 3 entries, with highlights numbered 3 and 4;
  - (c) an unlit scene with the player under a roof, and an object with one
    translucency bit in that scene, which draws its source pixels.
- **T18. Shroud rule (revision 3).** The predicate of section 3.3 gives every
  row of `rev3/shroud-mask.tsv`. On a synthetic map with an allowed set smaller
  than the grid, the recorder's image is 1 exactly where the predicate says,
  and the table's one row holds the dark index 256 times. If the recorder and
  the predicate differ, the painter wins: the implementer stops and reports
  the difference.
- **T19. No lifted refusal remains (revision 3).** With the renderer's refusal
  counters, the scenarios of T8, T10 and T17 record zero refusals for each of
  the four causes of section 3.16, and for a gray level above 7. Forcing a
  state that stays refused (T12, and a peek cap) still falls back.

## 5. Synthetic Test Fixtures (Hex / Text)

All values here are synthetic. Every file named below is in
`tests/fixtures/spec022/` or, for revisions 2 and 3, in its `rev2/` and
`rev3/` folders; each folder has a README. Team A computed every vector
from this spec with a throwaway script that is not committed.

### 5.1 Synthetic Lookup Tables

The scene does not use a palette. Its tables are defined by formula, so every
expected pixel can be checked by hand. `[Must-match]` for the fixtures only.

- Intensity: `I[0][k] = 0`; for i ≥ 1, `I[i][k] = min(255, trunc(i × k / 128))`.
- Additive mix: `X[a][b] = min(255, a + b)`.
- Common gray level: `s mod 8`. Glass gray level: `trunc(s / 8) mod 8`.
- Blend table t, with foreground colors 33, 35, 220, 58, 133 for t = 0 to 4:
  `F_t[0][D] = D`, and for g = 1 to 7,
  `F_t[g][D] = trunc((D × (7 - g) + color_t × g) / 7)`.

### 5.2 Sort Keys

`sort-keys.tsv` lists 14 keys; `rev2/sort-keys.tsv` keeps them and adds 5.
Worked rows:

| Item | Layer | Primary | Secondary | Key |
| --- | ---: | ---: | ---: | --- |
| Floor rank 9999 | 0 | 9999 | 0 | `0000270F00000000` |
| Upright, hex 20100, list index 1, part 0 | 2 | 20100 | 8 | `40004E8400000008` |
| Roof rank 5050, part 1 | 3 | 5050 | 1 | `600013BA00000001` |
| Shroud, draw rank 2 (revision 2) | 3 | 10000 | 2 | `6000271000000002` |
| Ground-item highlight 0, ring, with a full list: position 100 (revision 2) | 4 | 0 | 800 | `8000000000000320` |
| Ground-item highlight 0, ring, with a list of 1: position 1 (revision 3) | 4 | 0 | 8 | `8000000000000008` |
| Upright, hex 39999, list index 2^29 − 1, part 4 (revision 2) | 2 | 39999 | 4294967292 | `40009C3FFFFFFFFC` |

### 5.3 Hex Light

**Falloff world.** On elevation 0, light A is at hex 20100 with radius 2 and
intensity 65536 (step `trunc(64881 / 3) = 21627`). Light B is at hex 20104
with radius 3 and intensity 32768 (step `trunc(32113 / 4) = 8028`). They are
4 hexes apart. 49 hexes differ from 655. Selected rows:

| Hex | Distance to A | Distance to B | Accumulated | Entry | Level at ambient 32768 |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 20100 | 0 | 4 | 655 + 65536 = 66191 | 65536 | 128 |
| 20101 | 1 | 3 | 655 + 43909 + 8684 = 53248 | 53248 | 104 |
| 20102 | 2 | 2 | 655 + 22282 + 16712 = 39649 | 39649 | 77 |
| 20103 | 3 | 1 | 655 + 24740 = 25395 | 25395 | 64 (ambient) |
| 20104 | 4 | 0 | 655 + 32768 = 33423 | 33423 | 65 |

The byte image's digest is `D45003EF0BA78613`. `light-pack.tsv` covers the
clamp (-12 gives 0, 65537 gives 65536), and `light-levels.tsv` the level for
each light source (a fixed light of 511 gives level 0, 512 gives 1, 65536
gives 128).

### 5.4 The Scene

A 24-by-16 canvas, filled with index `0x40` before the refresh, and refresh
rectangle R = (0, 0, 20, 16). Layer 4 instances clip to (0, 0, 24, 16).

**Uniform block** (`scene-uniforms.hex`): OX = -4, OY = 2, ambient 32768
(level 64), elevation 0, flags 3. Cutout at (9, 0), 6 by 5 (the synthetic
mask `egg` of `scene-sprites.txt`, not `egg.frm`).

Peek, revision 2 (`rev2/scene-uniforms.hex`): center pixel (5, 2), inner
radius 2 and ring 4 (given directly, not from a zoom), so outer = 6 and
`Q[b] = (32 + 4 × (b + 1))²`: 36², 40², ... 96² (`rev2/peek-thresholds.tsv`).
Revision 1's peek (center (88, 40) in 1/16 pixel, R0 = 32, Wr = 48) is
superseded.

**Light.** The falloff world of section 5.3: hexes 20101, 20102 and 20103
give levels 104, 77 and 64.

**Images** (`scene-sprites.txt`): 13 sprites and rings, the level image
`lvl_m`, and the mask `egg`. `ring_o` is the opaque ring of `obj_a` in color
`0x31`, and `ring_t` the paint counts of `obj_b`'s ring. Its four hole pixels
are painted twice, once by each pass.

**Instances** (`scene.tsv`), in key order:

| Id | Layer | Mode | Light | Notes |
| --- | ---: | --- | --- | --- |
| `floor_u` | 0 | `lit`, EXEMPT | fixed 40000 (level 78) | Index `E6` stays unlit |
| `floor_m` | 0 | `floor_map` | `lvl_m` | Index `E8` is lit: no EXEMPT |
| `flat` | 1 | `lit`, EXEMPT | hex 20103 | |
| `flat_x` | 1 | `lit`, EXEMPT | hex 20103 | At (21, 1): outside R, culled |
| `obj_a` | 2 | `lit`, EXEMPT | hex 20101 | List index 1 |
| `obj_b` | 2 | `translucent`, table 1 | hex 20101 | List index 2, over `obj_a`; glass gray |
| `wall_c.0` | 2 | `egg` | hex 20102 | The 5-by-3 part inside the cutout; index `F5` lit (no EXEMPT) |
| `wall_c.2` | 2 | `lit`, EXEMPT | hex 20102 | Bottom strip |
| `wall_c.4` | 2 | `lit`, EXEMPT | hex 20102 | Right strip |
| `roof_1` | 3 | `lit`, EXEMPT, PEEK | ambient | Partly cut by the peek |
| `roof_2.0` | 3 | `egg`, PEEK | ambient | Part inside the cutout |
| `roof_2.4` | 3 | `lit`, EXEMPT, PEEK | ambient | Right strip |
| `ring_o` | 4 | `plain` | none | |
| `ring_t` | 4 | `outline_blend`, table 4, gray 2 | none | Hole pixels blend twice |
| `text` | 5 | `plain` | none | Clipped at x = 20 |
| `nohex` | 6 | `lit`, EXEMPT | fixed 65536 | Clipped at x = 20 |

**Batches** (`scene-batches.tsv`): `[floor_u, floor_m, flat, obj_a]`,
read `[obj_b, wall_c.0]`, `[wall_c.2, wall_c.4, roof_1]`, read `[roof_2.0]`,
`[roof_2.4]`, then `[ring_o]` (a new clip), read `[ring_t]`, and
`[text, nohex]` (clip R again). That is 8 draw calls and 3 snapshot copies
for 15 drawn instances.

**Worked pixels** of `scene-expected.hex`:

- (6, 5): `obj_a` leaves `I[0x82][104] = 105`. `obj_b`'s `09` has glass gray
  level 1, so `F_1[1][105] = trunc((105 × 6 + 35) / 7) = 95`, and
  `I[95][104] = 77` (`0x4D`).
- (10, 2): `wall_c`'s `A0` gives s1 = `I[160][77] = 96`. The mask value is 1,
  so the result is `X[I[96][1]][I[0x40][127]] = X[0][63] = 63` (`0x3F`).
- (9, 9): `floor_m`'s `E8` at level 120 gives `I[232][120] = 217`. Then
  `ring_t` paints it once with table 4 and gray 2:
  `trunc((217 × 5 + 133 × 2) / 7) = 193` (`0xC1`).
- (5, 2): inside the peek disc, so `roof_1` leaves the base `0x40`. Then the
  opaque ring paints `0x31` at (6, 2).
- (1, 0) (revision 2): world pixel (5, -2) has b = `Bayer[2][1]` = 11, and
  d² = 16 + 4 = 20. It is in the ring, and 256 × 20 = 5120 < Q[11] = 80² =
  6400, so it is cut and keeps the base `0x40`. Revision 1 kept `roof_1`'s
  `0x60` there. (0, 1) is cut the same way: world pixel (4, -1) has b =
  `Bayer[3][0]` = 15, d² = 26, and 6656 < Q[15] = 96² = 9216. These two
  pixels are the only differences from revision 1's canvas.

**Peek samples** (`rev2/peek-samples.tsv`, revision 2): pixel (3, 2) has
d² = 4 = inner², so it is cut (the disc includes its boundary). Pixel (2, 2)
has b = 2 and d² = 9: 2304 ≥ 44² = 1936, so it is kept. Pixel (1, 2) has b =
8 and d² = 16: 4096 < 68² = 4624, so it is cut, farther out than (2, 2). Pixel
(11, 2) has d² = 36 = outer², b = 10 and 9216 ≥ 76² = 5776, so it is kept.
Pixel (12, 2) has |dx| = 7 > outer, so it is kept at step 3.

**Peek scale** (`rev2/peek-scale.tsv`, revision 2): at zoom 1.0, inner = 96,
ring = 48, outer = 144, Q[0] = 1584² = 2509056 and Q[15] = 2304² = 5308416.
At zoom 1.25, inner = `round(76.8)` = 77 and ring = `round(38.4)` = 38. At
zoom 0.0625 the radii 1536 and 768 are capped to inner = 1024 and ring = 0.

### 5.5 Square Order (revision 2)

`rev3/square-rank.tsv` ranks nine floor squares and the grid's four corners by
section 3.4 item 4, with SPEC-003 section 3.6's example translation OX =
-3680, OY = -430 (square 5050's floor corner is (256, 180) and its roof corner
(256, 84), as SPEC-003 states). Floor corner `x = OX - 16 + 48s + 32t`, `y =
OY - 2 - 12s + 24t`, with s = 99 − column and t = row.

| Square | Floor corner | Rank (revision 3) | Among these nine (revision 2) | Ascending-index position (revision 1) |
| ---: | --- | ---: | ---: | ---: |
| 4950 | (224, 156) | 4974 | 0 | 0 |
| 5049 | (304, 168) | 5025 | 1 | 1 |
| 5050 | (256, 180) | 5074 | 2 | 2 |
| 5051 | (208, 192) | 5124 | 3 | 3 |
| 5149 | (336, 192) | 5125 | 4 | 6 |
| 5052 | (160, 204) | 5173 | 5 | 4 |
| 5150 | (288, 204) | 5174 | 6 | 7 |
| 5053 | (112, 216) | 5223 | 7 | 5 |
| 5250 | (320, 228) | 5274 | 8 | 8 |

5051 and 5149 share corner y 192, and 5052 and 5150 share 204: corner x
decides. 5149 and 5150 come before 5053, although their indexes are larger.
The roof corners are 96 pixels higher and give the same ranks. The four corner
squares have ranks 0 (square 0), 2500 (square 99), 7499 (square 9900) and 9999
(square 9999).

### 5.6 The Second Scene (revision 2)

A 12-by-8 canvas filled with `0x40`, and a whole-view refresh R = (0, 0, 12,
8), so every instance has clip R. It stands for an unlit scene: no item is
lit. `rev2/scene2-uniforms.hex`: OX = OY = 0, ambient 32768, elevation 0,
flags 1, cutout at (2, 1), 4 by 3 (the mask `egg2`), no peek. The images are
in `rev2/scene2-sprites.txt`, and the instances in `rev3/scene2.tsv`, out of
key order. For the `remap` instance, the `levels` column names its remap
table.

| Id | Key | Mode | Notes |
| --- | --- | --- | --- |
| `floor2` | `0000000000000000` | `plain` | 12 by 8, index `0x30 + x + 4y` |
| `obj2.0` | `4000006400000000` | `egg`, light `none` | Hex 100, list index 0, the part inside the cutout |
| `obj2.1` to `obj2.4` | `4000006400000001` to `...04` | `plain` | The strips around the cutout |
| `shroud` | `6000271000000000` | `remap`, table `shr_tbl` | Code 1 darkens (`I[D][32]`), code 2 writes index 0 |
| `ring0` | `8000000000000000` | `plain` | Outline list position 0 |
| `hl` | `8000000000000008` | `plain` | Ground-item highlight at position 1, because the list holds one entry (`rev3/scene2.tsv`; revision 2 gave it position 100, key `8000000000000320`). Drawn over `ring0` |

**Batches** (`rev2/scene2-batches.tsv`): `[floor2]`, read `[obj2.0]`,
`[obj2.1, obj2.2, obj2.3, obj2.4]`, read `[shroud]`, `[ring0, hl]`: 5 draw
calls and 2 snapshot copies.

**Worked pixels** of `rev2/scene2-expected.hex`:

- (3, 1): `obj2`'s `E9` is not relit under light source `none`, so s1 =
  233. The mask value is `0x40` = 64 and D = `0x37` = 55:
  `X[I[233][64]][I[55][64]] = X[116][27] = 143` (`0x8F`). The shroud leaves
  row 1 alone.
- (4, 2): s = `0x9E`, mask value `0xFF` used as 128, so the egg result is
  `X[158][I[D][0]] = 158`. The shroud's code 1 then gives `I[158][32] = 39`
  (`0x27`).
- (11, 2): the shroud's code 2 writes index `0x00`, which `plain` could not
  write, because a source 0 is transparent.
- (9, 1): `ring0` paints `0x31`, then `hl` (position 1, a later key)
  paints `0x3A` over it.

### 5.7 Gray-Level Bound (revision 2)

`rev2/blend-clamp.tsv` uses a synthetic gray rule that can exceed 7, as a
hand-built ink can (no palette can; section 3.6): the common gray level is
`s mod 16` and the glass gray level `trunc(s / 16) mod 16`. The blend tables are those of section 5.1, and D =
100. Every raw level above 7 is used as 7:

| Table | s | Raw gray | Used | `F_t[g][100]` |
| ---: | --- | ---: | ---: | ---: |
| 0 | `0F` | 15 | 7 | 33 |
| 0 | `03` | 3 | 3 | `trunc((100 × 4 + 33 × 3) / 7)` = 71 |
| 1 | `F0` | 15 | 7 | 35 |
| 4 | `0C` | 12 | 7 | 133 |

Mode 3 then applies `I[...][L]`; at L = 128 the formula table leaves the
value unchanged. For mode 5, the clamped level is what aux bits 8-10 hold.

### 5.8 Revision 3 Vectors

**Highlight positions** (`rev3/highlight-positions.tsv`). With n list entries,
highlight k has position `n + k` and secondary `8 × (n + k) + part`:

| n | k | Part | Position | Key |
| ---: | ---: | ---: | ---: | --- |
| 1 | 0 | 0 | 1 | `8000000000000008` |
| 7 | 2 | 1 | 9 | `8000000000000049` |
| 100 | 0 | 0 | 100 | `8000000000000320` |
| 100 | 5 | 1 | 105 | `8000000000000349` |

**Peek center** (`rev3/peek-center.tsv`). A pointer at (10.75, 20.25) gives
pixel (10, 20): the disc does not move by a fraction of a pixel. A pointer at
(-0.25, 3.5) gives (-1, 3), because floor rounds down, not toward zero. A
pointer at (700.5, 490.25) over a tile with corner (640, 480) gives (60, 10).

**Ring boundary** (`rev3/peek-ring.tsv`), at zoom 1.0: inner 96, ring 48. At
d = 99, `256 × d²` is 2509056, exactly `Q[0]` = 1584², so a pixel of b = 0 is
kept, and `Q[1]` = 2663424 cuts a pixel of b = 1. Here t = 3 / 48 and
`floor(16 × t)` = 1, so `b < 1`, as the second form says. At d = 144 every b
is kept, and `Q[15] = 256 × 144²`.

**Tile seam** (`rev3/peek-seam.tsv`). The view has center pixel (20, 2), inner
2, ring 4, OX = -4 and OY = 2, and a tile seam at x = 16. View pixel (17, 2)
is pixel (17, 2) of tile 0 and pixel (1, 2) of tile 1. Tile 1 has center
(4, 2) and OX = -20. In both, world pixel (21, 0) has b = 8 and d² = 9, and
2304 < 68² = 4624, so it is cut in both.

**Shroud** (`rev3/shroud-mask.tsv`). World pixel (0, 0) has b = 0 and threshold
400: at d² = 399 it stays, and at d² = 400 it takes the dark index. World
pixel (0, 3) has b = 15 and threshold 80² = 6400, the largest. At d² = 10815,
which is past 80 pixels, it is dark for every b. An allowed hex never changes,
and an off-grid hex is always dark.

**Unlit scenes** (`rev3/unlit-modes.tsv`). The scene of section 5.6 already
shows an unlit `egg`. Its strips are `plain`, as the file says. A translucent
object in that scene would also be `plain`.

## 6. Decisions

Every open point of the brief is settled here.

- **D-1 (owner instruction).** A GPU-instanced sprite batching and hex-light
  pipeline for `vault_render` (`vault_render::layers`,
  `vault_render::map::enhanced`) with 100% pixel parity by default with the
  8-bit indexed renderer, and fewer draw calls through instanced quads.
- **D-2 (owner instruction).** A deterministic sort order across floor tiles,
  flat scenery, standing objects and critters, roof tiles
  (`vault_core::roof`) and floating text, so that batching never changes
  occlusion or Z order.
- **D-3 (owner instruction).** A per-hex light buffer over the 200-by-200
  grid, clamped to `[0, 65536]`, and the roof X-ray peek parameters, passed
  to the GPU layer pass.
- **D-4 (owner instruction).** `cargo xtask capture-gallery && git diff
  --exit-code docs/assets` stays byte-identical in the default parity mode.
  Headless render tests use synthetic fixtures in `tests/fixtures/spec022/`.
- **D-5 (owner instruction).** Every rule tagged, and a Decisions section
  that separates owner instructions from Team A defaults.
- **D-6 (Team A default).** Parity is measured on the 8-bit indexed canvas,
  not on resolved colors, because the display stage is shared. The tags
  compare the instanced path with the reference path, as SPEC-020's compare
  the engine with itself.
- **D-7 (owner instruction; a Team A default in revision 1, confirmed by the
  owner in the Wave 3 prompt).** `reference` stays the default render path,
  in the shell, in replays and in the gallery. `instanced` is opt-in
  (`render_path`, `--render-path`). Making it the default is for the owner,
  once T11 has been recorded on real hardware and the owner's corpus.
- **D-8 (Team A default).** The instanced path never re-decides a
  `[May-improve]` item of SPEC-001 or SPEC-014. It copies the reference
  path's choice: the `EXEMPT` flag, the floor-light defects, and the cutout
  failures.
- **D-9 (Team A default).** The brief's five groups are extended with the
  other SPEC-014 section 3.44 layers: outlines between roofs and floating
  text, and no-hex objects last. "Flat scenery" is the whole flat pass, and
  "standing objects/critters" the whole upright pass, walls included,
  because SPEC-014 orders them that way.
- **D-10 (owner instruction, Wave 4 prompt: adopts a measurement by the implementer, Wave 3).** The square
  rank is the reference path's square order: ascending corner y, then corner
  x, then square index, with SPEC-003 section 3.6's floor or roof corner.
  Revision 1's Team A default (ascending square index) was wrong and is
  withdrawn. The parity rule did not change: the reference order wins.
  Revision 3 keeps this order and changes only the numbers (D-34).
- **D-11 (owner instruction; a Team A default in revision 1, confirmed by the
  owner in the Wave 3 prompt).** The brief's "ambient + point light falloff" is
  read as "both terms, combined by SPEC-006 and SPEC-014's maximum rule", not
  as a sum. A sum would break parity. The buffer stores the local light, 655
  base included, and the ambient travels in the uniform block, so a change of
  ambient light re-uploads 128 bytes, not 160000.
- **D-12 (Team A default).** Integer textures, `textureLoad` and integer
  arithmetic only. No filtering or float color math, so the GPU result
  cannot depend on the adapter.
- **D-13 (Team A default).** Draws that read the canvas go through snapshot
  copies, and read batches hold only disjoint rectangles. WebGPU has no
  portable read-back of the render target inside a draw.
- **D-14 (Team A default).** Outlines are drawn from ring images that the
  reference outline painter records: final indexes for opaque outlines,
  paint counts for translucent ones. That keeps SPEC-014's two-pass painting,
  its clipping and its buffer guards in one place.
- **D-15 (Team A default).** Floor level images come from the reference
  floor-light routine on the CPU, because its defects make each tile depend
  on the one before. A GPU compute pass is allowed only with equal values.
- **D-16 (owner instruction, Wave 4 prompt: adopts a measurement by the implementer, Wave 3).** Peek: the
  reference path's rule of section 3.10. Inner radius `round(96 / zoom)` and
  ring `round(48 / zoom)` in whole canvas pixels, with the zoom in interface
  pixels and no UI scale; the center at the cursor's canvas pixel, measured
  corner to corner; an inclusive inner disc; the ring test `256·d² ≥
  (16·inner + (b + 1)·ring)²` with the standard Bayer value b at the world
  pixel. Revision 1's Team A default (1/16-pixel fixed point, `(t + ½)`
  thresholds, pixel-center distances) is withdrawn. The reference path still
  wins any later difference.
- **D-17 (Team A default).** A cutout splits an item into at most five
  parts, numbered 0 to 4, so that the parts have distinct keys in a fixed
  order. The parts do not overlap, so their order changes no pixel.
- **D-18 (Team A default).** Any instanced-path failure falls back to the
  reference path for that refresh, as a full-canvas refresh. GPU tests skip
  without an adapter unless `VAULT_REQUIRE_GPU=1`, because CI hosts may have
  no GPU.
- **D-19 (Team A default).** The scene fixture uses formula tables instead of
  a palette, so every expected pixel can be checked by hand. The real tables
  are covered by the parity tests T10 and T11.
- **D-20 (Team A default).** Revision 1 has only the parity mode. A
  non-parity look, such as smooth per-pixel object light, needs a spec
  revision and must be opt-in under the enhanced preset.
- **D-21 (Team A default).** The interface names of section 3.14 and the
  setting name `render_path` are Team A's.
- **D-22 (Team A default).** The uniform block's bytes are fixed and tested,
  because they are the interface the brief names. The instance record is
  only recommended, because its atlas fields depend on the implementation.
- **D-23 (Team A default).** `render_path` is not logged or saved, so it can
  never change a replay or a save.
- **D-24 (Team A default).** A reference-path draw into the canvas that no
  draw mode can express refuses the instanced path for that view, and is
  reported for a revision. It is never approximated.

Revision 2 adds:

- **D-25 (Team A default).** The 64-bit key layout is kept. The enhanced
  shroud takes layer 3 with primary 10000, a value that no roof square rank
  reaches, so it sorts after every roof square and before every outline,
  where the reference path draws it. Revision 1's keys do not change.
- **D-26 (Team A default).** The shroud is drawn with a new read mode,
  `remap` (code 6): a per-pixel code that selects a 256-entry row, recorded
  by the reference shroud painter. Any per-pixel function of the canvas index
  is reproduced exactly, so the slot is parity-exact without this spec
  knowing how the shroud looks. A shroud draw that depends on more than the
  pixel's own index, or needs more than 255 rows, stays a refusal
  (`[Safe rule]`).
- **D-27 (Team A default; numbering superseded by D-35).** Ground-item
  highlights take outline positions after the outline list, in the reference
  path's highlight order, so they follow the whole list as the reference draws
  them. The position domain grows to below 2^29, the list index's bound.
  Revision 2 numbered them `100 + k`.
- **D-28 (Team A default).** The cutout of an unlit scene is the `egg` mode
  with light source `none`, under which `lit(s, L)` is s. No new mode is
  needed, and the cutout's blend formula is unchanged.
- **D-29 (owner instruction, Wave 4 prompt: adopts a measurement by the implementer, Wave 3).** The list index counts only
  the current elevation's entries, because the engine keeps one list per
  elevation. The reference path always redraws the whole view, and the
  enhanced canvas tiles are separate views; T10 covers tiles instead of
  partial refreshes.
- **D-30 (owner instruction, Wave 4 prompt, relayed by the coordinator;
  `[May-improve]`).** The gray level that selects a blend-table row is
  clamped to 0..=7 on the reference and instanced paths alike. The
  implementer measured that palette-derived gray levels never exceed 7
  (five-bit components; (63, 63, 63) gives 7), so parity and every rendered
  output are unaffected; the bound only keeps a hand-built ink with gray 8
  or more from indexing past a table's 8 rows.
- **D-31 (Team A default).** Revision 2's vectors go in
  `tests/fixtures/spec022/rev2/`, and the superseded revision 1 files stay
  until the implementer's follow-up switches the tests (section 3.15), as
  SPEC-021 revision 2 did.
- **D-32 (Team A default).** The revision 2 scene gives its peek radii
  directly (inner 2, ring 4), not through a zoom, so that the ring has 4
  canvas pixels on a 24-by-16 canvas. `peek-scale.tsv` covers the zoom math
  separately.

Revision 3 adds:

- **D-33 (Team A default).** The owner's brief of 2026-10-30 names this work
  "Revision 2", but revision 2 was approved on 2026-10-27 and has been built
  on. It is numbered revision 3, so that the history stays true and no approved
  text is rewritten. The content is the brief's. Where the brief's items were
  already in revision 2 (the ring formula, the square order, the object list,
  whole-view refreshes, the gray clamp), they are kept and completed, not
  restated as new.
- **D-34 (owner instruction, relaying the implementer's measurement of
  2026-10-30; numbering a Team A default).** The brief ranks squares by corner
  y, corner x and square index. The rank's number is the position among all
  10000 squares, as in the reference path's draw log, so that T9 compares
  numbers and not only order. Revision 2 counted drawn squares only. Skipped
  squares leave gaps, which the key allows.
- **D-35 (Team A default, adopting the implementer's measurement of
  2026-10-30).** A highlight's position is the running number `n + k`, as in
  the reference path's draw log and in the instanced path today. Revision 2's
  `100 + k` agreed only when the list was full. Keys of a full list do not
  change. This supersedes D-27's numbering.
- **D-36 (Team A default).** The shroud stays mode `remap`, although the
  measured painter does not read D. A new overwrite mode would save one
  snapshot copy per refresh, but it would change a decided mode table, the
  batch plan of section 5.6 and its fixtures. The `plain` mode cannot do the
  job, because the dark index may be 0, which is transparent in `plain`.
  `remap` with one constant row is exact, and already specified.
- **D-37 (owner instruction for the cutout, adopting the implementer's
  measurement of 2026-10-30; `plain` a Team A default).** In an unlit scene the
  cutout is `egg` with light source `none`, as D-28 says. Every other lit item
  uses the existing `plain` mode, which draws s, so no new mode is needed. The
  translucency bits are ignored there, because the reference path ignores
  them.
- **D-38 (Team A default).** The peek's safe bounds belong to the instanced
  path alone, because the reference path has none. Drawing from a capped radius
  would break parity at a zoom the shell cannot reach, and a guessed pixel is
  what this spec's `[Safe rule]` forbids. So a cap or clamp that changes a value
  refuses the refresh. Revision 2's cap values and `rev2/peek-scale.tsv` stand.
- **D-39 (Team A default).** The brief asks to lift four refusals, and
  `docs/REFERENCE.md` lists them. The instanced path's refusal of a gray level
  above 7 is a fifth, which section 3.15 step 3 already asked to remove. The
  checklist removes it too, as a separate step, and does not count it among
  the four. The `[May-improve]` tag of D-30 is unchanged.
- **D-40 (Team A default).** Revision 3's vectors go in
  `tests/fixtures/spec022/rev3/`, and the superseded revision 2 files stay
  until the implementer's follow-up, as D-31 did. Team A changed no file of
  an earlier revision.
- **D-41 (owner instruction).** Section 3.16 is an explicit checklist with the
  parity test for each refusal, modeled on SPEC-021 section 3.9. The default
  render path does not change (D-7).

## 7. Clean-Room Verification Checklist

- [x] Zero C/C++ code snippets or struct definitions included.
- [x] No implementation-source file path (the `CLAUDE.md` ingestion rule),
  and no 40-character hexadecimal string. Engine code is named only by Rust
  module path.
- [x] Every multi-byte integer's width and endianness stated (sections 2.3
  and 2.4).
- [x] Every rule tagged `[Must-match]`, `[May-improve]` or `[Safe rule]`;
  tables are tagged once and their rows inherit the tag.
- [x] Synthetic test vectors only. The light values follow from SPEC-006
  section 3.6's formula, and the digests from SPEC-021's FNV-1a 64.
- [x] Revision 2: every new vector was computed from this text by a
  throwaway script that first reproduced every revision 1 vector from the
  revision 1 text; the measured findings are labelled "measured by the
  implementer, Wave 3".
- [x] Revision 3: every `rev3/` vector was computed from this text by a
  throwaway script. A second, separate script then read only the written files
  and recomputed them: square ranks by counting rather than sorting, keys from
  layer, primary and secondary, the peek and shroud tests from their formulas,
  and the seam from both tiles (79 rows). The two forms of the ring test were
  also checked to agree for every whole-pixel d² and b at seven radius pairs.
  The measured findings are labelled "measured by the implementer, 2026-10-30".
- [x] Revision 3 keeps the ingestion rule: no implementation-source path, no
  40-character hexadecimal string, and no code listing. No engine Rust code was
  read.
- [x] Team A computed every vector in section 5 from this spec with a
  throwaway script that is not committed.
- [x] Decisions separate owner instructions from Team A defaults.
