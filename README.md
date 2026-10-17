# Vault Engine

A Rust clean-room engine workspace. Phase 0 establishes the crate boundaries,
loose-directory asset access, deterministic core primitives, and graphics shell.
Phase 1 (`P1-02` / `P1-03`) adds approved PAL, FRM/FR0–FR5, LST, FON, AAF, and MSG
parsers, format auditing, and an interactive sprite/font/message preview.

`AGENTS.md` is the repository's implementation policy. Format behavior follows
`specs/approved/SPEC-001-pal-frm-lst.md` and `SPEC-002-fonts-and-msg.md`. DAT
archives, bytecode execution, and gameplay await later phases. No original game
data is needed to build, test, or run the synthetic shell and preview fixtures.

## Workspace

| Package | Responsibility |
| --- | --- |
| `vault-formats` | VFS, palette/sprite/list/font/message parsers |
| `vault-core` | Generational entity handles, command/event IDs, save envelope |
| `vault-vm` | Library scaffold for the bytecode VM |
| `vault-guidance` | Library scaffold for progression and diagnostic hooks |
| `vault-render` | Integer index texture, palette lookup, sprite/text composition |
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
```

The CLI prints total mounted files and counts for `.frm`, `.pal`, `.fon`, `.aaf`,
`.pro`, `.map`, `.int`, `.msg`, and `.lst`, including zero counts. It traverses the
VFS namespace without parsing file contents. Invalid arguments or mounts exit
unsuccessfully with a diagnostic. With `--validate-formats`, the tool additionally
parses every `.pal`, `.frm`, `.fr0`–`.fr5`, `.lst`, `.fon`, `.aaf`, and `.msg`,
prints per-format pass/fail counts, and reports failing paths, formats, byte
offsets, and reasons on stderr. Any failure produces a nonzero exit after the
remaining assets have been checked. Phase 0's `loose/` markers are deliberately
not valid binary formats; use `tests/fixtures/formats/materialize.py` for a valid
synthetic corpus.

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

## Format safety and compatibility

All parsers forbid unsafe code and accept at most 64 MiB per asset. Audit/preview
reads enforce this limit before allocation and during reads. Font metrics are
limited to 4096 pixels, FON directories to 256 byte codes, and text tables to
1,000,000 entries. Diagnostics include the format and byte offset. Tests exercise
all section-4 fixtures from both specifications, their prescribed mutations,
every truncated binary prefix, and deterministic mutation/random-byte fuzz cases.

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
