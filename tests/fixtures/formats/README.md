# Approved Phase 1 fixtures

These fixtures transcribe **every** input in section 4 of approved SPEC-001 and
SPEC-002. No game data is included. Sparse hex files use `start: bytes` and
inclusive `start-end: repeated-byte` records. Expansion verifies that every byte
is specified; there is no implicit padding.

| Specification | Fixture | Verification |
| --- | --- | --- |
| SPEC-001 §4.1 | `palette.hex` | RGB scaling, invalid entry, transparency, discriminator/tables |
| SPEC-001 §4.2 | `shared.frm.hex` | Shared directions, FR0–FR5 layout, timing, anchor/hotspot |
| SPEC-001 §4.3 | `animated.frm.hex` | Packed blocks, all frames/directions, displacement |
| SPEC-001 §4.4 | `art.lst`, `proto.lst`, `scripts.lst` | Positional indexing, metadata, consumer IDs, LF/CRLF |
| SPEC-002 §4.1 | `font.fon.hex` | MSB-first rows, padding, metrics, limits, mutations |
| SPEC-002 §4.2 | `font.aaf.hex` | Coverage, placement, space, metrics, grayscale blend, mutations |
| SPEC-002 §4.3 | `messages.msg` | Multiline fields, replacement, false comments, LF/CRLF |
| SPEC-002 §4.4 | `message-bytes.json` | Every exact byte vector, raw encoding and NUL behavior |
| SPEC-002 §4.5 | `message-boundaries.json` | Every acceptance/failure case, including partial retained catalogs |
| SPEC-002 §4.5 | `message-lengths.txt` | Prefix `{30}{}{`, N copies of `41`, suffix `}` |
| SPEC-002 §4.5 | `message-eof.hex` | Archive versus Windows loose-file Ctrl-Z semantics |

`support.rs` expands these files for unit/integration tests and constructs the
specified grayscale palette subtest. Tests reject every truncated prefix of each
complete binary fixture, exercise malformed numeric/offset fields, and run a
deterministic random-byte and mutation fuzz smoke test. This is bounded regression
coverage, not a claim of exhaustive fuzzing.

Additional synthetic compatibility regressions:

- `palette-base.hex`: a complete 33,536-byte base PAL without an extension tag.
- `split.frm.hex`: two frames in one shared sequence (28 stored bytes), retaining
  an unsplit declared area of 150 bytes. This deliberately is not a sixfold
  multiple, so tests cannot pass by dividing a header total by a hardcoded six.
- `messages-compatible.msg`: padded and signed IDs, empty IDs, comment-like
  invalid fields, a multiline malformed ID, extra fourth fields, unmatched braces,
  missing fields, an unterminated record, and an overflowing ID. Valid messages
  after each defect must survive. All text is newly authored synthetic content.

The original SPEC-002 failure-boundary vectors run through the explicit strict
API. Default corpus-compatible loading is tested separately, including diagnostic
buffer limits and recovery after an oversized field.

`materialize.py` writes the same synthetic assets into an empty directory for
manual audit and GPU preview. It uses the grayscale palette subtest so text is
visible; `palette.hex` deliberately maps all colors back to zero. The extra
`preview.msg` record contains only `AB A B`, the drawable letters in these fonts.

```sh
python3 tests/fixtures/formats/materialize.py --output target/synthetic-corpus
cargo run -p asset-audit -- --corpus-dir target/synthetic-corpus --validate-formats
cargo run -p vault-platform -- --corpus-dir target/synthetic-corpus \
  --preview-frm art/items/animated.frm --font font0.aaf \
  --msg text/english/game/preview.msg --message-id 1
```
