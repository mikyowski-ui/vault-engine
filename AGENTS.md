# Vault Engine — Clean-Room Builder Agent Instructions

## Clean-Room Mandate
- You are a Clean-Room Builder Agent implementing a modern Rust engine from approved binary/behavioral specifications located in `specs/approved/`.
- NEVER reference, search for, or copy decompiled C/C++ source code from original Fallout executables or reverse-engineering repositories.
- Test all parsers and systems using synthetic fixtures committed in `tests/fixtures/`.

## Spec Authoring (Team A)
Authorized by the project owner on 2026-10-25.

- **When.** A roadmap item needs a new or revised specification (`SPEC-015` to `SPEC-019`, or any later spec), and no research-vault file is attached. The coordinator may then spawn a dedicated **Spec-Author Sub-Agent (Team A)**. It drafts and approves `specs/approved/SPEC-XXX-*.md` and the synthetic fixtures the spec describes.
- **Sources Team A may use:**
  - public format specifications (for example the IETF RFCs);
  - standard engine documentation;
  - the project's own approved specs;
  - the owner's briefs;
  - its general knowledge of publicly documented file formats and game behavior.
- **Never allowed:**
  - reading, searching for, quoting or paraphrasing decompiled or disassembled code, or re-implementation source (`fallout1-re`, `fallout2-ce`, `sfall` and the like);
  - proprietary game assets;
  - file paths under a `src/` directory;
  - 40-character hex hashes.
- **Open questions never halt the work.** Team A resolves each one with a sensible, documented clean-room default, and records it in the spec's **Decisions** section. Each decision says whether it is an owner instruction or a Team A default.
- **What every Team A spec carries:**
  - a **Provenance** line: the author (Team A), the date, the source categories used, and any owner brief;
  - the usual `[Must-match]` / `[May-improve]` / `[Safe rule]` classification;
  - test vectors made from synthetic data only.
- **Separation.** The implementing agent (Team B, or the coordinator) is a different agent from the spec's author. It implements from the approved spec alone, and the ingestion rule in `CLAUDE.md` still applies to every spec.
- **Recording.** `PROVENANCE.md` records which specs came from Team A.

## Workspace Architecture
This Cargo workspace has 10 packages: 7 core crates, 1 dev-only fixture crate and 2 CLI tools. `fuzz/` is a separate workspace with its own `Cargo.lock`, excluded from this one.
1. `crates/vault-formats`: VFS (`Vfs` trait, `LocalDirProvider`, `DatProvider`, `Dat2Provider`, `MemoryProvider`, and `LayeredVfs` for mod layers), DAT and DAT2 archives with LZSS and zlib, and parsers/writers for `.pal`, `.frm`/`.fr0`–`.fr5`, `.lst`, `.fon`, `.aaf`, `.msg`, `.pro`, `.map`, `.gam`, `.gcd`, `SAVE.DAT`, `AUTOMAP.DB`, `ai.txt`, `.acm`, `.mve`, movie fade `.cfg`, `.sve`, `.rix`, `.lip`, credits, `fallout.cfg`, the Fallout 2 tables and saves, and `vault-mod.toml` manifests, plus the FID codec, sound-name builders, and hex geometry. The file-backed code sits behind the default `host-fs` feature (SPEC-023).
2. `crates/vault-vm`: `.INT` loader (`IntProgram`), stack-based bytecode virtual machine, opcode dispatcher, the per-game word sets (`WordSet`), and the sfall words and arrays (SPEC-024b).
3. `crates/vault-core`: Deterministic world state and host extensions, state and combat hashes (SPEC-021), generational handles (`slotmap`), routing, animation sequences, turn/AP combat and combat AI, roofs, click policy, world travel, legacy save slots, the Fallout 2 party, sfall and mod script hooks, and UI state (`ui`, SPEC-020).
4. `crates/vault-guidance`: Pip-Boy holodisk lists and pages and quest status (SPEC-008 §3.8), derived from campaign globals and messages, and the Automaps page grouping.
5. `crates/vault-render`: 256-color indexed palette shaders, sprite batching, hex-tile camera, and bitmap font rendering via `wgpu`, with the CPU reference path and the GPU-instanced path that must match it (SPEC-022).
6. `crates/vault-debug`: Headless VM stepper and instruction trace, GVAR/MVAR/LVAR watches, and map object/critter inspectors with plain-text rendering for overlays.
7. `crates/vault-platform`: Desktop shell: windowing (`winit`), input mapping, audio output (`cpal`, the default `audio` feature), and main loop, with `SessionRuntime` (SPEC-020), the launcher and boot menu, touch UI, portable storage (`StorageBackend`, SPEC-023), replays, mod layers, and the documentation gallery.
8. `crates/vault-fixtures` (dev-only): the dependency-free synthetic test fixture builders from `tests/fixtures/`, compiled once and shared as a dev-dependency (a normal dependency of `xtask`).
9. `tools/xtask`: the gate runner (`check`, `wasm-check`), `clean-room-check`, the synthetic fixture writer, `export-mod` and `capture-gallery`.
10. `tools/asset-audit`: local corpus counts, format validation and audits.

## Engineering Gates
Every PR must pass (`cargo xtask check` runs all four):
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `cargo xtask wasm-check`: `vault-vm`, `vault-formats`, `vault-guidance` and `vault-core` compile for `wasm32-unknown-unknown` (SPEC-023 §3.17). Where the target is missing it skips with a stated line and checks them on the host; `VAULT_REQUIRE_WASM=1`, set in CI, makes a missing target a failure.
