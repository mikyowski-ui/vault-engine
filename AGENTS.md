# Vault Engine — Clean-Room Builder Agent Instructions

## Clean-Room Mandate
- You are a Clean-Room Builder Agent implementing a modern Rust engine from approved binary/behavioral specifications located in `specs/approved/`.
- NEVER reference, search for, or copy decompiled C/C++ source code from original Fallout executables or reverse-engineering repositories.
- Test all parsers and systems using synthetic fixtures committed in `tests/fixtures/`.

## Workspace Architecture
This Cargo workspace consists of 7 core crates plus CLI tools:
1. `crates/vault-formats`: VFS (`Vfs` trait, `LocalDirProvider`, `DatProvider`), DAT archives and LZSS, and parsers/writers for `.pal`, `.frm`/`.fr0`–`.fr5`, `.lst`, `.fon`, `.aaf`, `.msg`, `.pro`, `.map`, `.gam`, `.gcd`, `SAVE.DAT`, `AUTOMAP.DB`, `ai.txt`, `.acm`, `.mve`, movie fade `.cfg`, `.sve`, `.rix`, credits and `fallout.cfg`, plus the FID codec, sound-name builders, and hex geometry.
2. `crates/vault-vm`: `.INT` loader (`IntProgram`), stack-based bytecode virtual machine, and opcode dispatcher.
3. `crates/vault-core`: Deterministic world state and host extensions, generational handles (`slotmap`), routing, animation sequences, turn/AP combat and combat AI, roofs, click policy, world travel, and legacy save slots.
4. `crates/vault-guidance`: Pip-Boy holodisk lists and pages and quest status (SPEC-008 §3.8), derived from campaign globals and messages.
5. `crates/vault-render`: 256-color indexed palette shaders, sprite batching, hex-tile camera, and bitmap font rendering via `wgpu`.
6. `crates/vault-debug`: Headless VM stepper and instruction trace, GVAR/MVAR/LVAR watches, and map object/critter inspectors with plain-text rendering for overlays.
7. `crates/vault-platform`: Desktop windowing (`winit`), input mapping, audio output, and main loop.
8. `tools/xtask` & `tools/asset-audit`: CLI verification and local corpus auditing tools.
9. `crates/vault-fixtures` (dev-only): the dependency-free synthetic test fixture builders from `tests/fixtures/`, compiled once and shared as a dev-dependency (a normal dependency of `xtask`).

## Engineering Gates
Every PR must pass (`cargo xtask check` runs all three):
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
