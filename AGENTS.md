# Vault Engine — Clean-Room Builder Agent Instructions

## Clean-Room Mandate
- You are a Clean-Room Builder Agent implementing a modern Rust engine from approved binary/behavioral specifications located in `specs/approved/`.
- NEVER reference, search for, or copy decompiled C/C++ source code from original Fallout executables or reverse-engineering repositories.
- Test all parsers and systems using synthetic fixtures committed in `tests/fixtures/`.

## Workspace Architecture
This Cargo workspace consists of 7 core crates plus CLI tools:
1. `crates/vault-formats`: VFS (`Vfs` trait, `LocalDirProvider`, `DatProvider`) and binary asset parsers (`.pal`, `.frm`, `.lst`, `.fon`, `.aaf`, `.msg`, `.pro`, `.map`, `.int`).
2. `crates/vault-vm`: Stack-based `.INT` bytecode virtual machine and opcode dispatcher.
3. `crates/vault-core`: Deterministic world state, hex grid math, generational handles (`slotmap`), turn/AP combat rules, and save envelope.
4. `crates/vault-guidance`: Quest/progression tracker and headless diagnostic hooks.
5. `crates/vault-render`: 256-color indexed palette shaders, sprite batching, hex-tile camera, and bitmap font rendering via `wgpu`.
6. `crates/vault-debug`: Inspector overlays and asset browser utilities.
7. `crates/vault-platform`: Desktop windowing (`winit`), input mapping, audio output, and main loop.
8. `tools/xtask` & `tools/asset-audit`: CLI verification and local corpus auditing tools.

## Engineering Gates
Every PR must pass:
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
