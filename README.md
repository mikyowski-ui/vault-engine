# Vault Engine

A Rust clean-room engine workspace. Phase 0 establishes the crate boundaries,
loose-directory asset access, deterministic core primitives, and graphics shell.
Phase 1 (`P1-02` / `P1-03`) adds approved PAL, FRM/FR0–FR5, LST, FON, AAF, and MSG
parsers, format auditing, and an interactive sprite/font/message preview.
Phases 2 and 3 Core (`P2-02`, `P2-03`, `P3-02`) add PRO/MAP parsing, isometric
geometry, the INT loader/core VM, and map preview.
Phase 3 Host and Phase 4 (`P3-03`, `P4-02`, `P4-03`) add the host opcode catalog,
GAM/GCD resources, world state, character statistics, shuffled RNG, and interactive
script-driven dialogue. The SPEC-007 phase adds the complete art-identifier codec,
hex routing, rays/perception, and combat resolution and turns, and closes the
remaining SPEC-001–006 behaviors (see "Spec completion notes" below). The SPEC-008
phase adds DAT archives and LZSS, selected-database lookup, legacy save slots with
the working saved-map set and automap database, world travel, and Pip-Boy status
and rest.

`AGENTS.md` is the repository's implementation policy. Format behavior follows
`specs/approved/SPEC-001-pal-frm-lst.md` through
`SPEC-008-dat-saves-and-worldmap.md`.
No original game
data is needed to build, test, or run the synthetic shell and preview fixtures.

## Workspace

| Package | Responsibility |
| --- | --- |
| `vault-formats` | VFS, asset/prototype/map parsers, projection and picking |
| `vault-core` | Authoritative world, host services, stats/RNG, dialogue, save foundations |
| `vault-vm` | INT loader, two-stack core interpreter, explicit host interfaces |
| `vault-guidance` | Pip-Boy holodisk and quest status from campaign globals |
| `vault-render` | Indexed sprite/text/map/dialogue composition and palette rendering |
| `vault-debug` | Library scaffold for inspectors and asset browsing |
| `vault-platform` | Desktop shell, asset preview, input command logging |
| `xtask` | Engineering gate runner |
| `asset-audit` | Loose-corpus counts and format validation |

## Build and verify

Use Rust 1.88 or newer, with Cargo, rustfmt, and Clippy installed. `Cargo.lock`
pins the workspace dependencies. Standard checks need no desktop, GPU, or corpus.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# Run all three gates:
cargo xtask check
```

On Linux the desktop shell needs a Wayland or X11 session, the corresponding
window-system libraries (including libxkbcommon), and a Vulkan or OpenGL driver.
Windows and macOS use winit/wgpu's native backends.

## Desktop shell

```sh
cargo run -p vault-platform
# Present a frame and exit, failing if setup/presentation fails:
cargo run -p vault-platform -- --smoke-test
```

The window displays a centered 16×16 synthetic sprite containing all 256 indices,
scaled to whole pixels where space permits. An `R8Uint` texture holds indices;
WGSL resolves them through a separate 256-entry RGB332 palette texture. The
sprite retains its aspect ratio on resize. Minimized windows suspend rendering;
lost/outdated surfaces are reconfigured.

Keyboard, pointer, wheel, focus, modifier, resize, and quit observations are
written as JSON Lines to stdout. Command IDs begin at 1 in each session:

```json
{"command_id":1,"kind":"key","physical_key":"KeyA","pressed":true,"repeat":false}
```

Pointer coordinates and resize dimensions are physical pixels; wheel commands
specify `lines` or `pixels`. Physical keys use winit key-code names. Escape or the
window close button exits. Operational diagnostics are structured JSON on stderr,
controlled by `RUST_LOG` (default `info`). `WGPU_BACKEND` can select a graphics
backend for troubleshooting. Input order is sequenced deterministically; actual
OS event delivery is external to the simulation.

## Loose-directory VFS and corpus audit

```sh
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game
# Committed synthetic example:
cargo run -p asset-audit -- --corpus-dir tests/fixtures/loose
# Validate every supported asset and report all failures:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-formats
# Resolve every PRO/MAP art identifier through the SPEC-007 codec:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-art-ids
# Round-trip every PRO/MAP through the SPEC-003 writers:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-writers
# SPEC-008: enter every map, round-trip SAVE.DAT, and write/read a temporary slot:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-saves
```

`--validate-saves` checks that the campaign initializer covers every fixed
SPEC-008 campaign index (towns, entrances, quests, disks, encounter globals),
enters each map through the world host so it joins the working saved-map set,
requires every working `.SAV` to reparse with the saved bit, requires the central
`SAVE.DAT` to be a byte-exact parse/write fixpoint, and writes then restores a
save slot in a temporary directory.

`--validate-art-ids` reports, per source, identifiers whose decoded file exists,
decodable identifiers whose file is absent from the mounted corpus (`MISSING`
lines; not fatal, since the reference guarantees no fallback resource), and
identifiers the codec rejects (`FAIL`; nonzero exit). `--validate-writers` requires
PRO output to equal the consumed input with the documented weapon-bound exchange
and 24-byte scenery suffix, and MAP output to be a parse/write fixpoint that keeps
every saved object and retained script.

The CLI prints total mounted files and counts for `.frm`, `.pal`, `.fon`, `.aaf`,
`.pro`, `.map`, `.int`, `.msg`, and `.lst`, including zero counts. It traverses the
VFS namespace without parsing file contents. Invalid arguments or mounts exit
unsuccessfully with a diagnostic. With `--validate-formats`, the tool additionally
parses every `.pal`, `.frm`, `.fr0`–`.fr5`, `.lst`, `.fon`, `.aaf`, `.msg`,
`.pro`, `.map`, `.int`, `.gam`, and `.gcd`,
prints per-format pass/fail counts, and reports failing paths, formats, byte
offsets, and reasons on stderr. Any failure produces a nonzero exit after the
remaining assets have been checked. Phase 0's `loose/` markers are deliberately
not valid binary formats; use `tests/fixtures/formats/materialize.py` for a valid
synthetic corpus.

MAP validation resolves item/scenery state trailers through the mounted
`proto/<category>/<category>.lst` and `.pro` files. Missing dependencies are
reported as map validation failures. INT validation parses images without
executing game extensions.

Mount the directory whose immediate children include `art`, `data`, `maps`,
`premade`, `proto`, `scripts`, `sound`, `text`, root `color.pal`, and font files
such as `font0.aaf` and `font5.fon`. Folder names and extensions are not an
allowlist: partial corpora and other regular files are supported.

```rust
use vault_formats::{LocalDirProvider, Vfs};

fn main() -> Result<(), vault_formats::VfsError> {
    let assets = LocalDirProvider::mount("tests/fixtures/loose")?;
    let palette = assets.read("CoLoR.PaL")?;
    let sprite = assets.read(r"ART\ITEMS\test.frm")?;
    for path in assets.files() {
        println!("{path}");
    }
    Ok(())
}
```

Paths are relative ASCII names, normalized to lowercase with `/` separators.
Both slash styles are accepted in queries. Absolute paths, drive/UNC paths,
`.`/`..`, repeated/trailing separators, control characters, Windows special
characters/device names, and components ending in dots or spaces are rejected.
Unicode names are rejected to keep case folding identical across host platforms.

Mounts reject symlinks inside the tree, special files, and case collisions in
either files or directories. Reads check for replacement symlinks and use
`cap-std` directory capabilities, so confinement holds even across filesystem
races and renames of the mount's ambient path. A mount snapshots names, returns
them in lexical order, and reads current contents on demand. Remount after
adding/renaming files; stable file contents require an unchanged corpus.

Keep local proprietary data outside the repository or in ignored `corpus/`.

## Sprite, font, and message preview

```sh
cargo run -p vault-platform -- --corpus-dir /path/to/unpacked/game \
  --preview-frm art/items/example.frm
# Explicit overlay selection, also compatible with --smoke-test:
cargo run -p vault-platform -- --corpus-dir /path/to/unpacked/game \
  --preview-frm art/items/example.fr3 --font font0.fon \
  --msg text/english/game/example.msg --message-id 100
```

Left/right arrows wrap orientations 0–5. Up/down arrows step forward/backward
through animation frames, with wrapping. The window title shows the current
orientation/frame. Directional suffixes select the initial orientation while
retaining all six header slots. Escape exits. Preview images use nearest-neighbor
scaling and rectangle centering, which intentionally differs from world-anchor
placement. Sample text and the selected message are composited through the loaded
font and palette; index zero is transparent in sprite drawing.

The viewer loads root `color.pal`. It defaults to `font0.aaf`, then `font0.fon`,
then another root AAF/FON in lexical order (AAF preferred). It selects the first
MSG below `text/` in VFS order and its lowest ID with nonempty text. Explicit
missing/invalid assets or message IDs produce an error. Text remains raw bytes
mapped to font glyphs; the viewer does not assume UTF-8 or change languages.

## Map preview

```sh
cargo xtask fixtures --output target/state-corpus
cargo run -p asset-audit -- --corpus-dir target/state-corpus --validate-formats
cargo run -p vault-platform -- --corpus-dir target/state-corpus \
  --preview-map maps/synthetic.map
# The second fixture opens a no-head dialogue during startup for GPU verification:
cargo run -p vault-platform -- --corpus-dir target/state-corpus \
  --preview-map maps/dialogue.map --smoke-test
```

The fixture output directory must be empty. `--preview-map` and `--preview-frm`
are mutually exclusive. Arrow keys pan, **R** toggles roofs, and **0**, **1**, **2**
select elevations. Hover records on stdout include hex, floor-square, roof-square,
and elevation coordinates; picking reverses the displayed image's scale and
letterbox offset before applying the specification's inverse projection.

Click an object to select/look at it; right-click a critter to talk. The action
strip and **L/D/U/P/T** keys route look, description, use, pickup, and talk through
the authoritative world and object scripts. Approaches move one hex per animation
step and run spatial checks. Pickup updates inventory and removes the world
representation only after insertion succeeds.

Dialogue uses the actual script reply/options and callback procedure indices.
Click an option or use **1–9**; **0** selects the documented dummy no-callback
choice. **Space/up/down** navigate reply pages, the wheel scrolls options, and
**Escape** closes dialogue before closing the application. Head animation continues
while the world background clock is suspended. The synthetic Guide demonstrates
two nodes with a real VM callback.

`--player-gcd <VFS_PATH>`, `--language <directory>`, and `--seed <integer>` select
the player sheet, language, and deterministic RNG initialization. Defaults are
a neutral character, English, and seed 1. Seed -1 requires explicit host entropy
and is not silently derived from a wall clock.

The map view uses a 640×380 logical viewport. Floor/roof images use the exact
square projection; roofs are 96 logical pixels above their floor. Objects use
hex centers, stored displacement, directional FRM shifts, and bottom-center
alignment. Tile images are not resized to the diamond footprint. Hidden objects,
hidden/blank tiles, and inventory children are excluded from placed-object drawing.
Flat objects precede upright objects; remaining object order is deterministic by
anchor position and ID. The complete indexed scene scales with the window.

Object and tile art resolves through the SPEC-007 codec (`ArtCatalog`): the FID's
own category selects the list, critters append the two-letter animation/weapon
suffix and the selector's `.frm`/`.fr0`–`.fr6` extension, heads append their
selector/variant, and other categories use the list name verbatim. A decodable
object FID whose file is absent is logged and not drawn; codec failures are errors.
If the requested frame is unavailable, frame zero is reported and displayed.
Cached map art is limited to 256 MiB. Floor index 1 is drawn like any floor (only
roof index 1 is exempt). Palette cycling runs in the shell and follows the world's
cycling state (for example, disabled during dialogue); script-loaded palettes
replace the display palette.

## Format safety and compatibility

All parsers forbid unsafe code and accept at most 64 MiB per asset. Audit/preview
reads enforce this limit before allocation and during reads. Font metrics are
limited to 4096 pixels, FON directories to 256 byte codes, and text tables to
1,000,000 entries. Diagnostics include the format and byte offset. Tests exercise
all section-4 fixtures from the approved specifications, prescribed mutations,
required-data truncation, and deterministic mutation/random-byte fuzz cases.

- `Palette` accepts the complete 33,536-byte RGB-plus-RGB555 base layout without
  a discriminator, as well as an optional four-byte tag/padding. A `CWEN` tag
  requires all three extended tables; a partial tag or table is rejected.
  Valid VGA channels multiply by four
  (63 becomes 252); invalid triples become unavailable black entries. Supplied
  tables are retained, and generated intensity is evaluated lazily. `PaletteCycler`
  implements the specified shared timers, rotations, red pulse, and reset rules.
- `FrmArchive` accepts arbitrary version words and preserves signed timing.
  Adjacent equal direction offsets share records. Defensive validation requires
  packed sequential blocks, positive counts/dimensions, and exact pixel sizes.
  The number of stored sequences comes from the shared offset slots; the header
  always has six slots, with no separate orientation-count field. Single-sequence
  split animations may retain a larger declared area from the unsplit animation.
  `stored_frame_area_len()` reports the actual bytes read. Undersized declarations
  and mismatches in multi-sequence files are rejected, along with unsafe
  gap/reordering/reuse cases.
  Trailing bytes after the frame area are ignored. World rectangles, cursor
  hotspots, preview centering, and cumulative displacement are separate operations.
- `LstTable` requires a consumer (`Art`, `Script`, `Prototype`, or `Raw`). Bounded
  line reads, blanks, comment-like lines, and duplicate names retain positions.
  Art/script/prototype lookups apply their distinct ID masks/bases. Unknown
  paths such as `data/*.lst` use raw physical lines without invented name rules.
  Embedded NUL and overflowing numeric metadata are rejected.
- `Font` retains bit-packed FON data and byte-level AAF coverage. It validates all
  drawable extents against the correct loaded block, rejects negative metrics
  and undefined AAF levels 14+, and preserves spacing, bottom alignment, space
  width, complete-advance clipping, and the legacy maximum-cell queries. There is
  no pair kerning. Rendering supports proportional/monospaced text, underline,
  shadow, and the specified palette blend levels, including 8–13.
- `MsgCatalog` preserves byte encodings, concatenates multiline fields, honors
  duplicate replacement, and supports merging. Default loading trims ASCII
  whitespace around signed decimal IDs and skips empty/invalid IDs, extra fields,
  and other malformed input while retaining earlier complete messages. Recovery
  resumes at an indented decimal ID followed by another opening field on the same
  physical line, so a broken record does not consume the following valid record.
  `warnings()` retains the first 128 diagnostics and `warning_count()` counts all
  recoveries. Fields remain capped at 1023 retained bytes, IDs at signed 32-bit
  range, and file/record resource-limit violations remain errors.
  `parse_strict`/`load_strict` retain the original SPEC-002 validation rules for
  authoring and regression tests. `parse` uses archive/raw Ctrl-Z semantics;
  `parse_with_mode(..., TextMode::WindowsLoose)` exposes the original Windows
  loose-stream EOF behavior. Both translate CRLF. Audit/preview consistently use
  raw VFS byte semantics on every host.

Base PAL acceptance, split-animation area metadata, and tolerant MSG recovery
implement the requested real-corpus compatibility corrections to the initial
strict interpretation of the approved specifications. Automated regressions use
synthetic fixtures; original corpus data is not included in tests.

The preview assumes same-coordinate destination sampling for AAF blends, as in
the approved synthetic fixture. The specification leaves original-executable
sampling order unresolved; this implementation does not claim to resolve it.

## Prototypes, maps, and geometry

`Prototype` dispatches all six categories from the PID's full high byte. Items
and scenery have unaligned subtype payloads, and tile/miscellaneous prefixes have
their own lengths. Every known subtype is typed; unknown subtypes are rejected.
`consumed_len` excludes tolerated trailing bytes, including the scenery writer
suffix. Basic, extended, critter, lock, container, map, script, and combat flags
have separate types and preserve uninterpreted bits.

`MapFile::parse` takes a `PrototypeLookup` provider. `LocalPrototypeCatalog` resolves
one-based prototype-list entries, caches subtype/default information, and uses
the selected file's PID to decode it. Maps retain embedded MVAR/LVAR pools for
later engine initialization; no campaign GVAR array or invented GAM parser is
involved. Negative header pool counts consume zero words. Tile omission flags
omit only the corresponding tile block, and roof-hide bits clear on loaded tiles.

All 16 script slots in each block are decoded using their individual category,
including padding slots. Active occupancy, local slices, and unique active script
IDs are checked. Object trees are consumed depth-first, quantities represent
one child tree, and enclosing elevations/owners are restored. Script bindings,
outlines, critter flags, stock ammunition, and exit-grid art receive the specified
loading adjustments. Obsolete pointer words never become offsets or live pointers.
Limits are 100,000 scripts, 100,000 total objects including children, and inventory
depth 64. Counts and actual read extents are checked before allocation.

`HexGrid` provides bounded neighbors, border-stop stepping, both distance
definitions, camera centering, and integer hex/floor/roof projection/picking.
Negative inverse coordinates use floor division. The recovered angular walk
returns an error when it leaves the grid or fails to converge, including the
documented `(0,199)` boundary defect; only the explicit `-1` sentinel gives 9999.

## Core VM

`IntProgram` validates the 42-byte executable bootstrap region, procedure records,
identifier/literal records and terminators, and complete instruction boundaries.
Procedure addresses are absolute image offsets and must select executable
instruction boundaries, with one loading exception: a procedure body/condition
may equal the exact image length to represent an unused trailing stub. Jumping,
calling, or evaluating a condition at EOF still raises a runtime address error;
addresses beyond EOF remain parse errors. Identifier-consuming operations resolve
their raw operand against the identifier table even if pushed with a string tag;
literal bounds are enforced when a value is consumed as a string. The image is
bounded to 64 MiB, procedure count to 65,536, and declared arguments to stack capacity.

`Vm` implements all 76 core dispatch slots, including distinct raw/type-dependent
truth rules, asymmetric integer-addition promotion, changing-limit discard,
float-bitwise conversion, context returns, mutable scheduling metadata, and the
recovered wait predicate. Both stacks have 2048 bytes and fit 341 tagged values.
Frames and globals use byte bases, with checked live-slot access. Strings are
interned per program with an 8 MiB content budget; cross-program transfers copy text.

`update` uses ten instructions by default. Critical state can continue beyond the
ordinary budget; a separate 100,000-instruction hard limit prevents an unbounded
critical invocation. Arithmetic cases whose results are undefined in the spec
(overflowing subtraction/multiplication/negation, invalid float-to-int conversions,
and nonfinite float-to-text conversions) return explicit VM errors.

`VmHost` supplies time, shared exports, program ownership/launching, wait predicates,
and extension opcodes `0x804C..=0x8155`. `BasicHost` supplies a deterministic clock
value and case-insensitive export registries. No process-global state or filesystem
access is implicit in execution. Host calls and external return contexts are
available explicitly; the unsafe imported-call argument-transfer path is rejected
with `ImportedAbi` because SPEC-004 does not define a safe executable ABI for it.

Lifecycle resolution preserves the generic index-1 fallback and the separate
map-enter/map-update presence checks. Scheduled conditions execute against the
real VM/host and can have side effects; no pure-procedure flag is invented.
Timed deadlines use strict unsigned comparison, condition flags take precedence,
and firing/canceling clears the specified metadata words.

## World state and host extensions

`GamFile` applies the exact 16-byte section comparisons and 257-byte translated
chunks. Names do not affect indices, only column-zero `//` is a comment, and later
section headings remain data. Missing markers yield an empty initializer. An
assignment with no decimal prefix or an overflowing integer is rejected instead
of exposing the reference's uninitialized/undefined value.

`GcdFile` decodes all 428 bytes with checked tags/traits, name termination, and
raw SPECIAL bounds. `player_normalized()` applies player-only EMP/body/reward/
kill-category changes; `CharacterSheet::from_gcd` then recomputes derived bases
and performs the bounded initial healing request.

`CharacterSheet` separates trait-adjusted raw attributes R from effective,
bonus-adjusted E. It implements all 16 trait selections, all listed stat/skill
perk effects, tag investment, difficulty, equipment/addiction effects, and
accumulated level-up HP history. Perk effects are applied on rank changes, not on
each query. `ParkMiller` exposes a complete shuffle-state snapshot, preserves
entry zero on reseeding, distinguishes seed/draw sign correction, and supports
explicit startup diagnostic draws. Checks preserve the tick-864000 boundary and
their exact number of random draws.

`WorldState` owns generational entities and separate opaque VM references,
GVAR/MVAR/LVAR storage, the ten-tick clock, ordered timed events, lighting,
script contexts, and dialogue. Script requests distinguish their first bootstrap
execution from later named calls. Map loading records and executes the ten
specified stages, including full ambient before entry and player light after
the initial map-script request. Timers use stable insertion order at equal due
times; spatial callbacks are checked on each animated movement step with reentry
suppressed. Inventory stack splits preserve a distinct removed representative.

The extension ABI is compiled from the approved SPEC-005 table at build time.
All 266 slots are present: 264 registered words and holes `0x807D`/`0x807E`.
`vault-vm::extensions` preserves raw tags, uses mask `0xF7FF`, implements the
I/S/F/U/O/N/C/T checks and variable region arity, and does not synthesize statement
results. `WorldHost` connects the catalog to world services and managed-library
state. Documented stubs, missing-result null paths, busy-state leaks, defective
tokenization, sound-handle tags, and wrong-string-tag paths are explicit.
Unsafe dereferences, nonterminating legacy paths, and undefined arithmetic are
reported as bounded errors.

Exports use separate 1013-entry registries. Variable ownership is by registered
program filename; same-owner registration resets to integer zero. Procedure
exports are removed on program purge, while variables survive until registry
cleanup. Named callbacks, keyboard bindings, and script timers retain their
separate lifetimes.

`GDialog` tracks session, node processing, review, paging, callbacks, and head
animation separately. Replies clear old options, IQ filtering happens at append
time, options are limited to 30, review replies to 80, and reply pages to ten.
Head/background IDs, filename suffixes, repeated-first-column fidget counts,
reaction transitions, and the 42 phoneme mappings follow SPEC-006. The platform
renders loaded head/background/interface art and font glyphs; no fabricated
conversation is substituted for a missing NPC script.

Host boundaries remain explicit where the approved specs do not define the
underlying tables or media formats. `WorldRules` supplies numeric perk mappings,
XP thresholds/bonuses, currency/medical/equipment PID roles, and numeric action
codes not enumerated by the specs. `WorldAssets` supplies sound-name codecs and
localized continuation labels. Absent mappings are diagnosed or rejected, rather
than guessed from reverse-engineering sources. Unobstructed light arithmetic is
exact; occlusion/shot lines use the engine's bounded integer hex-line policy.
Audio, movies, general managed-window drawing, combat requests, and unspecified
item/medical defaults are typed host presentation/action requests. The map preview
implements world interaction and GDialog rendering, not media decoding or a full
combat/medical subsystem.

## Art identifiers, routing, and combat (SPEC-007)

`Fid` packs base/weapon/animation/category/selector fields; `ArtCatalog` parses the
category lists and implements the exact critter suffix table (weapon codes 0-10
checked before every branch, single alias substitution for animations
27/29/30/33/55/57/58/64, weapon actions requiring a weapon), head suffixes, and the
constructor's directional fallback (only critter animations 20-35 except 33 probe
the requested selector, then `.fr0` once; the original base is retained).

`routing` implements the heuristic best-first route search exactly as recovered:
seen-on-insertion, first-vacant frontier slots, 50 per hex plus 10 per direction
change outside combat (initial direction 0), 1999-entry frontier/expanded caps,
and an 800-byte backtrack that keeps the last steps. The movement blocker covers
critter/scenery/wall art on the hex plus multihex neighbors; non-player
biped/robotic critters auto-open unlocked doors. Rays use pixel Bresenham between
hex centers with stride sampling, the 200-sample cap, and projectile-through
filtering at stride 32; the first eligible object on a hex masks the others.
`obj_can_see_obj` combines the sector perception rule with a stride-16 ray whose
first blocker must be the target (the ignored reference is explicitly null).
Move-to-object/tile planning shortens routes and derives the recovered replan
destination; execution opens usable doors, rebuilds blocked routes with a strict
goal, and charges 1/4/8 AP per hex (free movement first) in combat.

`combat` holds the explicit attack arithmetic: AP costs and ranges, the exact hit
chance (no lower cap; ammunition AC/DR/multiplier/divisor are not consumed), the
first-day-gated critical upgrade and forced upgrades, the 864 critical-success
entries and failure masks (compiled from the approved specification at build
time), per-round DT/DR damage, knockback with the double-decrement displacement,
burst/continuous sprays (budgets, direct trials, side rays, extra targets at
normal scale), death-animation selection with the inverted rear fallback, and
corpse rules. `combat_state` implements initialization, the first unsorted pass,
the round sweep, turns, explicit end requests, automatic ending, and cleanup.
Critical-failure self damage is computed but committed only to an attacker already
marked dead, as recovered. AI behavior is unspecified, so NPC turns and danger
targets come from a host-supplied `CombatAi` (default: none). The weapon-splash
helper adds no victims (its setup is absent in the reference), and the miss-impact
fallback beyond grenade scatter is not specified enough to simulate.

Registered animation sequences now execute: commands run in order, one frame step
at a time (the art's frame interval, 10 FPS without art); nonnegative delays wait
that many frame steps; moves follow SPEC-007 routes with unlimited AP; forward
death animations end on their static art; completed sequences retire (returning
critters to standing unless flag 4), so `anim_busy` reports `-1` only while a
nontrivial registration is active.

## Archives, saves, world travel, and Pip-Boy (SPEC-008)

`DatArchive` reads the byte-zero root and per-directory tables (one-byte name
lengths, big-endian words, `data_size` of at least 16) and extracts members by
attribute word: exactly zero or `0x10` is the legacy single LZSS stream whose
output must equal the logical length, `0x20` is raw, and `0x40` is the blocked
form (two-byte headers; every block must produce exactly
`min(16384, remaining)` bytes). The LZSS decoder uses the 4096-byte ring with
4078 leading spaces, cursor `0xFEE`, LSB-first control bits, absolute 12-bit
starts, and sequential overlapping copies. Truncated tokens, over-expansion,
short or empty blocks, and offsets past EOF are errors rather than unbounded
reads or loops. The reference leaves the ring tail uninitialized; it is zeroed
per file here (and retained across that file's blocks), which only matters for
streams that read the tail before writing it.
`Database` and `DatProvider` implement selected-database lookup: the loose patch
directory first, then that database's archive, with `art/critters/` selecting
the critter database and no fallback between databases. `VfsWorldAssets` and
`LocalPrototypeCatalog` accept any `Vfs`, so the world host runs from a loose
directory or from `DatProvider` archives. The desktop shell's `--corpus-dir`
still mounts a loose directory.

`savegame` reads and writes the strict 30051-byte header (only the first 18
signature bytes are compared; versions are not enforced) and all 27 ordered
payloads with no length prefixes. The campaign count G comes from the installed
initializer; the queue grammar uses type-dependent payloads; manifest names are
bounded to 12 bytes before the NUL; trailing bytes are rejected. `AutomapDb`
holds `MAPS/AUTOMAP.DB` (version 1, the 797-byte empty table with the excluded
map set, LZSS rasters that stop at 10000 output bytes, and the `X = 200 - column`
raster writer that spills column zero into the next row). Captures are stored as
raw entries, since the specification defines no encoder: rewriting an entry
shifts later offsets and the recorded length, and appending requires EOF to equal
the recorded length.

Leaving a map writes its `.SAV` image (saved bit set, live globals/locals,
scripts, and placed objects with inventories) into an in-memory working
saved-map set and captures its automap rasters. Entering a map prefers that image
over the asset. The player is saved centrally. Party members travel with the
player between maps but are written to the current map in a save slot. The
per-load map-script instance is recreated on each entry, as in SPEC-006 step 6,
so its previous local block is not reattached. `WorldHost::save_game` writes
`SAVEGAME/SLOTnn/SAVE.DAT`, the manifest's `.SAV` companions, and `AUTOMAP.SAV`,
renaming old files to `.BAK` first and restoring them if a write fails.
`load_game` reads and validates the header, every payload, every companion, and
the automap length before it changes any state, which is safer than the
reference's partial loads. It then restores in the reference order: reset, game
tick, campaign snapshot A, working set, current map, then the player and later
components (snapshot B is ignored). An active saved combat resumes the player's
turn without an AP refill. The saved next-identity counter is installed before
the map load, so temporary objects cannot take saved inventory identities.
Payloads the engine does not model are carried verbatim: preferences outside
difficulty, combat difficulty, violence, and burning-run; movies; skill
timestamps; interface words; the pending-perk byte; and ranks for unmapped perk
indices. Perk ranks map through `WorldRules::perk_ids`. The platform supplies
the thumbnail and host date/time (`SaveInfo`). The base calendar year is
`WorldRules::calendar_year` (default 0), because the specs define only the
non-leap month/day calendar.

`worldmap` compiles the terrain, frequency, and region grids, the town table,
and the hotspot catalog from the approved specification at build time. It
implements the asymmetric pixel stepper (the X-major branch can finish one Y
pixel short), binary32 W/Q travel speed (inputs that would divide by zero or go
negative are rejected), the terrain schedules on one shared counter, the
29-stride discovery reveal (3 x 3, or 5 x 5 with Scout, plus the west/ocean
thresholds), 3d6 encounter classes with town suppression, special encounters
(restarting the whole test when the drawn special is consumed), terrain-map
redraws, globals 65/32, the town-knowledge refresh (known value exactly 1,
byte-narrowed entrances, Vault 13's tick rule, alternate rows 12-14), buttons,
labels, and first entry. `WorldHost::travel_iteration` runs one loop iteration:
schedule steps and walk-mask stops, then the Q charge with queue interruption,
discovery every third iteration, then 24-hour party healing and the encounter
check every W iterations. Script-directed town travel skips encounters and the
mask and charges no city time. `vault-guidance` derives holodisk lists, pages,
and quest status from the fixed tables. Pip-Boy rest (`WorldHost::rest`)
implements eligibility, the A/N/K step allocation, queue-interrupted steps, the
strict final catch-up, credited minutes with three-hour healing at 180,
until-healed (including the extra chunk after full HP), and wake-up presets.

Where SPEC-008 leaves details open, this engine makes these choices:

- A desert/coast iteration resets the terrain counter to zero, and the mountain
  and city schedules treat a nonpositive counter as expired.
- Within an iteration, steps come before the time charge, discovery, and the
  interval check.
- Town and destination jitter uses one signed `-16..=16` draw per axis.
- Travel time charges never move the clock backwards.
- The walking-mask contents are host-supplied; without a mask nothing blocks.
- Automap capture uses objects' existing seen flags and runs at every map save.
- Town knowledge refreshes on town entry.
- Rest arithmetic uses binary64.
- Map-exit script request failures are diagnostics, like map-entry failures,
  so a missing compiled map script cannot trap transitions.

## Spec completion notes

These close behaviors that the earlier phases left open:

- SPEC-001: the gamma display formula (`display_channel`), an opaque display
  palette (index 0 is transparent only in blits), cycler fade suspension, and
  `FrameCursor` forward/reverse displacement accumulation.
- SPEC-002: `FontSet` startup search (FON 0-9, AAF 0-15), per-subsystem defaults,
  numbered selection (FON 0-9, AAF 100-110) that never replaces the current font
  with a missing slot, `setfont`, and the numeric text-flag word.
- SPEC-003: asymmetric `Prototype::write` and `MapFile::write` (darkness 1,
  recomputed omission bits, grid-order top-level saving without temporary or
  unplaced objects, `NO_SAVE` scripts omitted, temporary inventory children
  refused), extended-flag interaction permissions, and out-of-range MVAR/LVAR
  diagnostics. The roof-hide flood fill is named but not specified, and unknown
  item/scenery subtypes are still rejected rather than read without payload.
- SPEC-004: scheduled procedures fire from the host's interpreter pass, and
  `launch` starts attached, replacing, spawned, and independent programs.
- SPEC-005: executing animation sequences, armor-aware `critter_damage`, the exact
  `obj_can_see_obj`, fleeing-target rejection, `anim` 1000 controls, loaded
  palette tables, retained button graphics, named-event busy state, and
  consistent 31-byte export keys. Sound-name byte formats are not specified
  completely, so they remain a host codec.
- SPEC-006: elevation-change map updates, speech completion, the queue
  stop-on-nonzero rule, reputation inputs (campaign indices supplied by
  `WorldRules`), talk reachability/12-tile checks, campaign-open failure, and
  map-load and map-exit script failures as diagnostics. Map loads now consult
  the SPEC-008 working saved-map set before the asset lookup.

## Core contracts

`EntityStore<T>` is a `slotmap::SlotMap<EntityId, T>`. Construct it with
`EntityStore::with_key()`. Removal invalidates a handle even if the slot is reused;
handles are scoped to their owning store. Serde support preserves slot generations
when the store is serialized.

`SequenceIds` owns independent command and event counters. Each begins at 1,
uses checked increments, and returns an error after issuing `u64::MAX`. Persist
the counters with world state (Serde or `from_last`) to avoid reusing IDs after
restore. Allocation does not use clocks, global counters, or randomness.

The engine-owned save envelope has a fixed **24-byte little-endian** header:

| Offset | Size | Meaning |
| --- | --- | --- |
| 0 | 8 | ASCII magic `VAULTSAV` |
| 8 | 2 | Version, currently 1 |
| 10 | 2 | Header size, currently 24 |
| 12 | 4 | Reserved flags, must be zero |
| 16 | 8 | Payload byte length |

`SaveHeader::decode` validates the header prefix. `decode_envelope` also validates
the exact payload length, rejecting truncation and trailing bytes without
allocating from the declared length. Unknown versions, header sizes, and flags
are rejected. Payload encoding and integrity checks are for future phases; this
envelope does not claim compatibility with legacy game saves.
