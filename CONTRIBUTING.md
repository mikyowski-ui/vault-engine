# Contributing

Vault Engine is a clean-room implementation (see `PROVENANCE.md`). Its
legitimacy depends on every contribution coming from the approved
specifications alone. Please read this whole document before opening a pull
request.

## Clean-room self-certification (required)

Each pull request description must include this statement, unedited, and each
commit author must be able to make it truthfully:

> I certify that this contribution was written only from the approved
> specifications in `specs/approved/`, the oracle vectors in `oracle/`, and
> material already in this repository. I have not consulted, and did not use
> while writing it, any decompilation, disassembly, or reimplementation of the
> original game executables, or any reverse-engineering source repository. The
> contribution contains no original game assets.

Pull requests without the statement are not reviewed.

### What this means in practice

- **Do not read** decompiled or disassembled game code, or engine
  reimplementations or recreations of any kind, whether in this or any other
  language. This applies even "just to check something". If you have studied
  such material in depth, talk to the maintainers before contributing to the
  affected subsystem.
- **Do not run** decompilers, disassemblers, or debuggers against the original
  executables to answer an implementation question.
- **Do not commit** original game data in any form: files, extracted members,
  screenshots, audio, text excerpts, or fuzz corpora or crash inputs built
  from them. Fixtures must be synthetic and authored for this project.
- **When a spec is unclear or incomplete**, open an issue that asks for a spec
  revision. Do not guess from outside sources. The research side answers
  through an approved revision or a new oracle vector.
- **Reject tainted specs.** A spec that contains file paths under `src/`,
  40-character git commit hashes, or raw C decompilation listings must not be
  committed or edited locally. Report it so that a sanitized revision can be
  produced (see the ingestion rule in `CLAUDE.md`).

### Compatibility classes

- `[Must-match]` behavior (formulas, opcodes, binary and save layouts, LZSS
  framing, FID encoding, route costs) must match the spec and the oracle
  vectors exactly.
- `[May-improve]` behavior (reference memory bugs, uninitialized reads,
  unbounded loops) must never be reproduced as undefined behavior. Return an
  explicit `Result` error or apply the spec's safe rule, and document the
  choice in the README section for that spec.

## Engineering gates

Every pull request must pass:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`cargo xtask check` runs all three. `--locked` makes Cargo fail instead of
changing `Cargo.lock`; commit lockfile changes deliberately. CI runs the same
gates on Linux, checks the workspace on the minimum supported Rust version
(`rust-version` in `Cargo.toml`, 1.88), builds and tests on Windows and macOS,
and runs the `cargo audit` and `cargo deny` dependency checks over both
`Cargo.lock` and `fuzz/Cargo.lock`. A new dependency must pass both and use a
license allowed by `deny.toml`.

Every binary parser has a fuzz target in `fuzz/` (see `fuzz/README.md`). Add a
target for each new binary parser, and keep the targets building: CI checks
them on every pull request (`cargo check --manifest-path fuzz/Cargo.toml --bins
--locked`, which works on stable), and a scheduled nightly workflow builds them
with `cargo +nightly fuzz build` and runs each briefly.

## Pull request checklist

- [ ] The clean-room certification statement is in the description.
- [ ] Every change traces to a section of an approved spec, or is engine
      infrastructure that needs no spec.
- [ ] New behavior has tests on synthetic fixtures.
- [ ] The engineering gates pass locally.
- [ ] The README section for the affected spec records any engine choices
      made where the spec leaves details open.
