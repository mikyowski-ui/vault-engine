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
| `int` | `IntProgram::parse`, then a bounded run under `BasicHost` (64 budgeted updates with scheduled-procedure polls) |
| `save` | `SaveHeader::parse` and `SaveGame::parse` (campaign count 618) |
| `automap` | `AutomapDb::parse`, then `raster` for 160 maps x 3 elevations |
| `pro` | `Prototype::parse`, then `info` |
| `font` | `Font::parse_fon` and `Font::parse_aaf`, then every glyph lookup and `text_width` over the input |
| `msg` | `MsgCatalog::parse`, `parse_with_mode(WindowsLoose)`, and `parse_strict` |
| `lst` | `LstTable::parse` for every `LstKind`, then the ID lookups |
| `gcd` | `GcdFile::parse` |
| `gam` | `GamFile::parse` as the campaign file and as a map file |
| `rix` | `rix::Splash::parse` |
| `sve` | `sve::parse` |
| `moviefx` | `moviefx::parse`, then a `FadeHook` over 256 frames |
| `ai` | `ai::Config::parse` and `ai::Packets::parse`, then `resolve` |
| `validate` | `validate_asset`: the first input byte selects one of 16 asset paths (so the parser and its path-dependent rules), the rest is the asset |

The `map` and `save` targets resolve each PID from its own bits (type byte,
subtype from the low byte; `fuzz_targets/common.rs`), so every
prototype-dependent object layout is reachable without a prototype corpus.
The `acm` target skips inputs whose header would allocate more than 2^20
samples or a 2^18-element block, since the decoder's own 64 MiB bound is too
generous for a fuzzing loop. The import runs against a `WorldAssets` that has no
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
It defaults `--target` to the triple it was built for. A prebuilt cargo-fuzz
(for example the musl binary CI installs) therefore needs
`--target x86_64-unknown-linux-gnu`: AddressSanitizer cannot link a static libc.

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
cargo check --manifest-path fuzz/Cargo.toml --bins --locked   # what PR CI runs
cargo build --manifest-path fuzz/Cargo.toml --bins
fuzz/target/debug/dat fuzz/corpus/dat -runs=0
```
