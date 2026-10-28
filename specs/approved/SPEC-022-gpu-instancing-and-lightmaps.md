# SPEC-022: GPU-Instanced Sprite Batching and Hex Lightmaps

**Status:** approved by the Spec-Author sub-agent (Team A), 2026-10-27, under AGENTS.md "Spec Authoring (Team A)". Revision 1.

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
2. the logical **sprite instance** and its six **draw modes**, as exact
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
| 3 | Roof | Drawn roof squares | Square rank | Part |
| 4 | Outline | Outline rings, in outline-list order | 0 | 8 × outline-list position + part |
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
  list (SPEC-014 section 3.44, "Per-hex object lists"), counting every entry
  of every elevation. It is below 2^29.
- **Hex number** is 0 to 39999. **Square rank** is 0 to 9999.
- **Outline-list position** is 0 to 99. **Creation rank** is the text's
  position among the live floating texts, oldest first. **Reference draw
  rank** is the position in which the reference path draws the object among
  the no-hex objects.

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
| aux | u16 | Bits 0-2: blend table, 0 to 4. Bits 8-10: gray level, 0 to 7 (draw mode 5 only). Other bits are 0 |

The draw modes are one rule. "Reads the canvas" means that the result depends
on the canvas pixel D under the instance pixel. `[Must-match]`

| Code | Mode | Light source | Reads the canvas | Used for |
| ---: | --- | --- | --- | --- |
| 0 | `plain` | `none` | No | Opaque outlines, floating text |
| 1 | `lit` | `hex`, `ambient` or `fixed` | No | Lit floor squares with one light, objects, roofs, strips |
| 2 | `floor_map` | `level image` | No | Floor squares lit per pixel |
| 3 | `translucent` | `hex` or `fixed` | Yes | Objects with exactly one translucency bit |
| 4 | `egg` | `hex` or `ambient` | Yes | The parts of objects and roofs drawn through the player cutout |
| 5 | `outline_blend` | `none` | Yes | Translucent outlines |

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
| `0x30` | 4 | i32 | Peek center x, in 1/16 canvas pixel (section 3.10) |
| `0x34` | 4 | i32 | Peek center y, in 1/16 canvas pixel |
| `0x38` | 4 | u32 | Peek outer radius, in 1/16 canvas pixel |
| `0x3C` | 4 | u32 | Peek inner radius, in 1/16 canvas pixel |
| `0x40` | 64 | 16 × u32 | Peek squared thresholds, entry t at `0x40 + 4 × t` |

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
   `cargo xtask capture-gallery` (Decision D-7). `[Must-match]`
3. The instanced path has one lighting mode in revision 1, **parity**: its
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
one rectangle R of the canvas, and the canvas keeps its pixels between
refreshes. The instanced path runs the same refreshes, with the same
rectangles, in the same order, on a canvas that it keeps on the GPU between
refreshes. `[Must-match]`

For each refresh R:

1. **Base fill.** Fill R exactly as the reference path does before the floor
   layer, for example with a clear index or the off-grid shroud.
   `[Must-match]`
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

**Clip.** Every instance of layers 0, 1, 2, 3, 5 and 6 has clip R. Every
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
- **Outline (layer 4).** One instance for each entry of the refresh's
  outline list (at most 100), in list order. Its image is the outline's
  **ring image**: the reference path's outline painter (SPEC-014 section
  3.44, "Outline geometry"), run with all its clipping and buffer guards,
  but recording each paint instead of writing the canvas. The ring image
  covers the frame widened by one pixel on every side. `[Must-match]`
  - For an opaque outline, the ring pixel is the index of the paint that
    stays (the column pass's when there is one), and 0 where nothing was
    painted. The instance is mode `plain`.
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
   the order in which the reference path draws the squares of that layer.
   Revision 1 records that order as ascending square position
   `column + 100 × row`, so the rank is the square position itself
   (Decision D-10). If the reference path uses another fixed order, the rank
   is the position in that order. The implementer then states the order in
   the commit and asks for a spec revision. Either way the instanced path
   follows the reference path. `[Must-match]`
5. **Outlines, texts, no-hex objects** follow their list, creation and
   reference orders (section 2.1). `[Must-match]`
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
pixels of the item's frame. The `egg` part sets `EXEMPT` exactly when the
reference path applies SPEC-014 section 3.37's `[May-improve]` (indexes
229-255 unlit in the masked draw). When the mask art is missing, or the
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

The level is `L = clamp(E, 0, 65536) >> 9`, from 0 to 128 (SPEC-014's light
level divided by 512). A `level image` pixel gives L directly.

**Results.** Let `lit(s, L)` be s when `EXEMPT` is set and s ≥ 229, and
`I[s][L]` otherwise. With F_t the blend table t, `F_t[g][D]` its entry at
`256 × g + D` (SPEC-002 section 3.2), and `gray(s)` the glass gray level of
s for table 1 and the common gray level otherwise (SPEC-014 section 3.44):
`[Must-match]`

| Mode | Result |
| --- | --- |
| 0 `plain` | s |
| 1 `lit` | lit(s, L) |
| 2 `floor_map` | lit(s, L), with L the level image's pixel `level[level y + v][level x + u]` |
| 3 `translucent` | `I[F_t[gray(s)][D]][L]`, t = aux bits 0-2. No exemption for 229-255 |
| 4 `egg` | Let s1 = lit(s, L) and m the cutout mask's value at `(x - cutout left, y - cutout top)`, or 0 outside the mask. If m is 0 the result is s1, otherwise `X[I[s1][m]][I[D][128 - m]]` |
| 5 `outline_blend` | Start with r = D, then repeat s times: r = `F_t[g][r]`, with t = aux bits 0-2 and g = aux bits 8-10. The result is r. No light is applied |

A mask value above 128 is used as 128. `[Safe rule]`

Mode 5 applies the blend once for each paint, so a ring pixel painted twice
blends a second time over its own first result, as SPEC-014 section 3.44
says. `[Must-match]`

### 3.7 Batching

**Kinds.** Modes 0, 1 and 2 are **overwrite** modes. Modes 3, 4 and 5 are
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
constant on screen at every zoom. The parameters are computed on the CPU, in
`vault_render::map::enhanced`, and passed in the uniform block.

**Parameters.** `[Must-match]`

- **Screen sizes.** The inner radius is 96 screen pixels, and the ring is 48
  screen pixels wide (Decision D-16).
- **Scale.** `canvas_scale` is the number of screen pixels per canvas pixel
  in the enhanced view (the zoom, times any further scaling the view
  applies to the canvas). With `q(p) = floor(16 × p / canvas_scale + 0.5)`,
  evaluated in IEEE-754 double precision:
  - inner radius `R0 = q(96)`;
  - ring width `Wr = q(48)`;
  - outer radius `R1 = R0 + Wr`.

  All three are in 1/16 canvas pixel. If R1 would exceed 16384, R0 becomes
  `min(R0, 16384)` and Wr becomes `min(Wr, 16384 - R0)`. `[Safe rule]`
- **Center.** With (cx, cy) the cursor's position in canvas coordinates (a
  real number, pixel (x, y) spanning x to x + 1), the center is
  `(floor(16 × cx + 0.5), floor(16 × cy + 0.5))`, each clamped to
  -2^24 to 2^24. `[Safe rule]` for the clamp.
- **Thresholds.** For t = 0 to 15, `T[t] = R0 + trunc(Wr × (2t + 1) / 32)`.
  The uniform block holds `T[t]²`. Every T[t] is below R1, so each fits in a
  u32.

**Dither matrix.** The standard 4-by-4 Bayer matrix, row by row:
`[Must-match]`

```text
 0  8  2 10
12  4 14  6
 3 11  1  9
15  7 13  5
```

**Peek test** for canvas pixel (x, y): `[Must-match]`

1. World pixel `(wx, wy) = (x - OX, y - OY)`, so the pattern moves with the
   world, not with the canvas. `t = Bayer[wy mod 4][wx mod 4]`, with the
   non-negative remainder.
2. `dx = 16x + 8 - center x`, `dy = 16y + 8 - center y`.
3. If `|dx| > R1` or `|dy| > R1`, the pixel is not cut.
4. Otherwise the pixel is cut when `dx² + dy² < T[t]²`.

So every pixel nearer than R0 + Wr/32 is cut, every pixel at R1 or beyond is
kept, and between them a growing share of the dither cells keeps the roof.
Every value fits in an i32. A canvas is at most 16384 pixels wide
(section 3.11), so with the center's clamp |dx| and |dy| stay below 2^25.
Step 3 runs before any square, and after it |dx| and |dy| are at most 16384,
so `dx² + dy²` is at most 2^29.

**Parity.** This is revision 1's reading of the documented X-ray. The
reference path's X-ray defines the gallery's `roof-xray-peek.gif`, which must
not change (D-4). Test T8 compares the two. If they differ, the implementer
changes neither path's peek: the instanced path refuses refreshes with the
peek active, so the reference path draws them (section 3.12). The
implementer reports the reference path's peek rule, and its parameters, for
a spec revision. `[Safe rule]`

### 3.11 GPU Requirements

1. **Integer only.** The canvas is an `R8Uint` texture. Sprites, ring
   images, level images, the cutout mask, the gray tables, and the
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
   upload error, a draw no mode can express (section 3.2), or the peek
   mismatch of section 3.10. The refusal is a typed error. That refresh is
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

## 4. Required Tests

The implementer writes at least these tests. They use synthetic data only.
GPU tests need a wgpu adapter. Without one they print `skipped: no GPU
adapter` and pass, unless the environment variable `VAULT_REQUIRE_GPU` is
`1`, in which case they fail. Every other test runs everywhere.
`[Must-match]`

- **T1. Sort keys.** The key function gives every row of `sort-keys.tsv`.
  Layer 7, a hex of 40000, a square of 10000, a part of 5 in layers 1 to 3,
  a part of 1 in layer 0, a part of 2 in layer 4, and a list index of 2^29
  are each refused with an error.
- **T2. Scene order.** Reading `scene.tsv` (written out of order) and sorting
  by key gives the order of `scene-order.txt`. Each row's key equals the key
  computed from its layer, primary and secondary.
- **T3. Batch plan.** Culling and planning the scene gives
  `scene-batches.tsv`: `flat_x` is culled, and there are 8 batches, 3 of them
  read batches. The snapshot column gives the bounding boxes.
- **T4. Uniform block.** Packing the scene parameters (section 5.4) gives the
  128 bytes of `scene-uniforms.hex`.
- **T5. CPU executor.** A CPU executor implements sections 3.6 and 3.7
  exactly: it runs batches, and read batches through a snapshot. Running the
  scene on a 24-by-16 canvas filled with index `0x40` gives
  `scene-expected.hex`. A second run that draws the instances one at a time,
  without batches, gives the same canvas.
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
- **T8. Roof peek.** The parameter math gives `peek-scale.tsv` and
  `peek-thresholds.tsv`. The peek test gives every row of
  `peek-samples.tsv`. Then compare the reference path's X-ray with the
  instanced path's on the gallery's showcase map: the cursor at 3 positions
  and zooms 1.0, 1.25 and 2.0, roof peek `cursor_xray` held. If they differ,
  apply the section 3.10 parity rule.
- **T9. Order parity.** A draw-recording hook in the reference path logs
  each draw as (layer, hex or square, list index, part). For the synthetic
  maps (`maps/synthetic.map` from `cargo xtask fixtures --output DIR`,
  `maps/cam_demo.map` in `DIR-cam-demo`, and the gallery showcase map), the
  log equals the instanced path's sorted instance list. Cover at least: a
  walking critter (reinsertion on each frame step), two objects on one hex,
  a flat object, the player under a roof (cutout split), a hidden roof
  group, an outline, a floating text, and an object given each translucency
  bit through the engine's API.
- **T10. Canvas parity (CPU).** For every frame of T9's scenarios, the
  reference canvas equals the canvas of T5's executor run on the instanced
  path's instances. Cover a full refresh and at least one partial refresh,
  and an enhanced widescreen view at zoom 1.0 and 2.0.
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

## 5. Synthetic Test Fixtures (Hex / Text)

All values here are synthetic. Every file named below is in
`tests/fixtures/spec022/`, which has a README. Team A computed every vector
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

`sort-keys.tsv` lists 14 keys. Three worked rows:

| Item | Layer | Primary | Secondary | Key |
| --- | ---: | ---: | ---: | --- |
| Floor square 9999 | 0 | 9999 | 0 | `0000270F00000000` |
| Upright, hex 20100, list index 1, part 0 | 2 | 20100 | 8 | `40004E8400000008` |
| Roof square 5050, part 1 | 3 | 5050 | 1 | `600013BA00000001` |

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
mask `egg` of `scene-sprites.txt`, not `egg.frm`). Peek center (88, 40),
which is the center of pixel (5, 2); R0 = 32 and Wr = 48 (2 and 3 pixels),
so R1 = 80 and T = 33, 36, 39 ... 78 in steps of 3.

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

**Peek samples** (`peek-samples.tsv`): pixel (4, 2) has t = 0 and
dx² + dy² = 256 < 33², so it is cut. Pixel (2, 2) has t = 2 and 2304 ≥ 39² =
1521, so it is kept. At pixel (0, 0), |dx| = 80 is not above R1 = 80, so
the distance test applies: t = 3 and 7424 ≥ 42² = 1764, so it is kept.

**Peek scale** (`peek-scale.tsv`): at canvas scale 1.0, R0 = 1536, Wr = 768
and R1 = 2304. At 1.25, R0 = `floor(1228.8 + 0.5) = 1229` and Wr = 614.

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
- **D-7 (Team A default).** `reference` stays the default render path in
  revision 1, in the shell, in replays and in the gallery. `instanced` is
  opt-in (`render_path`, `--render-path`). Making it the default is for the
  owner, once T11 has been recorded on real hardware and the owner's corpus.
- **D-8 (Team A default).** The instanced path never re-decides a
  `[May-improve]` item of SPEC-001 or SPEC-014. It copies the reference
  path's choice: the `EXEMPT` flag, the floor-light defects, and the cutout
  failures.
- **D-9 (Team A default).** The brief's five groups are extended with the
  other SPEC-014 section 3.44 layers: outlines between roofs and floating
  text, and no-hex objects last. "Flat scenery" is the whole flat pass, and
  "standing objects/critters" the whole upright pass, walls included,
  because SPEC-014 orders them that way.
- **D-10 (Team A default).** The square rank is the reference path's square
  order, recorded as ascending square position. Overlap between floor or
  roof images is rare, but the order is still fixed. A different reference
  order changes the rank, not the parity rule.
- **D-11 (Team A default).** The brief's "ambient + point light falloff" is
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
- **D-16 (Team A default).** Peek: inner radius 96 and ring 48 screen
  pixels, read from `docs/REFERENCE.md`'s "a 96-pixel disc with a 48-pixel
  ring" as radius and ring width. Fixed point in 1/16 canvas pixel, integer
  squared thresholds, the standard 4-by-4 Bayer matrix, and world anchoring
  through OX and OY. If the reference path's X-ray differs, the reference
  path wins and the instanced path falls back for peek refreshes until a
  revision (section 3.10).
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
- [x] Team A computed every vector in section 5 from this spec with a
  throwaway script that is not committed.
- [x] Decisions separate owner instructions from Team A defaults.
