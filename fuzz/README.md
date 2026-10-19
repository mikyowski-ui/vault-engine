# Parser fuzzing

[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets for the binary
parsers. This crate is excluded from the main workspace (it has its own
`[workspace]` and `Cargo.lock`), so the standard gates neither build nor need it.

| Target | Entry points |
| --- | --- |
| `dat` | `DatArchive::parse`, then `extract` on every member; `DatTables::read` and `DatEntry::read_from` over a seekable stream |
| `lzss` | `dat::decode_blocks` and `dat::decode_until`; the first four bytes (little-endian, modulo 64 KiB + 1) select the output length |
| `frm` | `FrmArchive::parse` |
| `pal` | `Palette::parse` |
| `acm` | `acm::decode` |
| `mve` | `mve::Movie::open`, then `next_event` until `Finished` or an error |
| `map` | `MapFile::parse`, then `vault_core::WorldState::import_map` on every successful parse |

The `map` target resolves each PID from its own bits (type byte, subtype from
the low byte), so every prototype-dependent object layout is reachable without a
prototype corpus. The import runs against a `WorldAssets` that has no
prototypes, programs, or messages, so missing resources must be reported, not
assumed.

Every target only calls the API and discards the result. An `Err` is the
expected outcome for most inputs; a panic, abort, timeout, or out-of-memory
report is a bug. The `mve` target also fails if the event loop is still
running after 2^20 events, which would mean it stopped advancing.

## Prerequisites

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
```

## Seeds

The starting corpora come from the committed synthetic fixtures
(`tests/fixtures/` and `oracle/SPEC-012/synthetic/*.mve`). From the repository
root:

```sh
python3 -I fuzz/seed.py   # writes fuzz/corpus/<target>/
```

`fuzz/corpus/`, `fuzz/artifacts/`, and `fuzz/coverage/` are gitignored. **Never
put real game assets in a corpus that is committed or shared**, including
inputs that libFuzzer derived from them. Seeds must be synthetic fixtures or
minimized crash inputs built from them. Real assets may be used only in a local,
uncommitted corpus.

## Running

```sh
cargo +nightly fuzz list
cargo +nightly fuzz run dat
# Bounded run, for example in CI:
cargo +nightly fuzz run mve -- -max_total_time=60
# Cap input size and memory:
cargo +nightly fuzz run map -- -max_len=65536 -rss_limit_mb=2048
```

`cargo fuzz` runs from the repository root and finds this crate by itself.

## Crashes

libFuzzer writes a failing input to `fuzz/artifacts/<target>/`. Then:

```sh
# Reproduce:
cargo +nightly fuzz run <target> fuzz/artifacts/<target>/crash-<hash>
# Shrink it:
cargo +nightly fuzz tmin <target> fuzz/artifacts/<target>/crash-<hash>
# Shrink the corpus:
cargo +nightly fuzz cmin <target>
```

When reporting a crash:

1. Fix it in the parser so it returns an explicit `ParseError` or applies the
   spec's safe rule. Reference engine bugs are never reproduced as undefined
   behavior.
2. Add the minimized input as a regression test in the parser's `*_tests.rs`.
   Use the hex fixture style of `tests/fixtures/` when it is synthetic.
3. Never attach an input derived from real game data to an issue or commit.

## Checking without nightly

Stable Rust can type-check and build the targets. Without nightly they have no
coverage instrumentation, so this only proves that they compile and that the
seeds run:

```sh
cargo check --manifest-path fuzz/Cargo.toml --bins
cargo build --manifest-path fuzz/Cargo.toml --bins
fuzz/target/debug/dat fuzz/corpus/dat -runs=0
```
