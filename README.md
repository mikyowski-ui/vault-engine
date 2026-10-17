# Vault Engine

A Rust clean-room engine workspace. Phase 0 (`P0-03` / `P0-04`) establishes the
crate boundaries, loose-directory asset access, deterministic core primitives,
and a runnable synthetic graphics shell.

`AGENTS.md` is the repository's implementation policy. `specs/approved/` is
currently empty, so this phase implements the requested engine foundations.
Legacy binary parsers, DAT archives, bytecode execution, and gameplay await
approved specifications. No original game data or reverse-engineered source is
needed to build, test, or run the shell.

## Workspace

| Package | Phase 0 responsibility |
| --- | --- |
| `vault-formats` | `Vfs`, normalized `VfsPath`, `LocalDirProvider` |
| `vault-core` | Generational entity handles, command/event IDs, save envelope |
| `vault-vm` | Library scaffold for the bytecode VM |
| `vault-guidance` | Library scaffold for progression and diagnostic hooks |
| `vault-render` | Integer index texture, 256-entry palette, WGSL rendering |
| `vault-debug` | Library scaffold for inspectors and asset browsing |
| `vault-platform` | Desktop executable, lifecycle, input command logging |
| `xtask` | Engineering gate runner |
| `asset-audit` | Loose-corpus extension counts |

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
```

The CLI prints total mounted files and counts for `.frm`, `.pal`, `.fon`, `.aaf`,
`.pro`, `.map`, `.int`, `.msg`, and `.lst`, including zero counts. It traverses the
VFS namespace without parsing file contents. Invalid arguments or mounts exit
unsuccessfully with a diagnostic.

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
