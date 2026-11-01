# SPEC-024c Oracle Vectors

Vectors backing `SPEC-024c-mod-premade-startmap-and-city-rules.md` section 6. Every sheet, INI block, and table here is synthetic. `table-measured.tsv` holds counts taken from mod data, and no mod bytes.

| File | Contents |
| --- | --- |
| `gcd-layout.tsv` | The 440-byte layout, with each field's offset in all three layouts |
| `gcd-g1.hex` to `gcd-g5.hex` | Complete synthetic sheets: 16 spaced bytes a line, so they can be read by hand |
| `gcd-vectors.tsv` | G1 to G8: length, FNV-1a 64 of the file, and the decoded fields, or `refused` |
| `gcd-effect.tsv` | A1 to A6: what taking a sheet does to `HApStyle` and the stored style |
| `ddraw-vectors.tsv` | D1 to D47: one INI text, one field, the expected value, and the diagnostic count |
| `fixture-ddraw.ini`, `fixture-ddraw-expected.tsv` | One whole startup block and everything it should give |
| `fixture-maps.txt`, `fixture-city.txt` | Two small tables that show every rule of section 4 |
| `table-vectors.tsv` | 46 rows: those fixtures read by SPEC-018 as written, and by SPEC-024c |
| `table-measured.tsv` | Section 4.5's table: Et Tu and Restoration Project Updated under three rule sets |

## Reading the files

- `gcd-*.hex`: remove the spaces and line ends to get the sheet. Words are big-endian. The name starts at line 24 of a 440-byte sheet, because `0x174` is `16 x 23 + 4`.
- `gcd-vectors.tsv`: `-` means the layout has no such field. FNV-1a 64 is as in SPEC-021 section 2.1, and G6 to G8 hash their refused bytes.
- `ddraw-vectors.tsv`: `|` in the INI text stands for a line end. `(no file)` means no `ddraw.ini`. `unchanged` means the existing rule stands.
- `table-vectors.tsv`: read the rows by `rules`. The `SPEC-018 as written` rows are the fault, and they show that a test can fail. The `SPEC-024c` rows are the requirement.
- The fixture's remaining `SPEC-024c` diagnostics are real faults, kept on purpose: a header with no `]`, a repeated lookup name with its repeated stem, two eight-item entrances that need one, and one unknown map.
- `gcd-effect.tsv`: `no write` means sfall global `HApStyle` is not touched.

## Verification

The generator's checks stop it from writing on any failure:

- The critter-stat portion is 368 bytes, and all three layouts decode one sheet the same way.
- The worked ddraw results hold: `V13ent`, tick 264600, tick 863400, 25 July 2241, and 5 December 2161.
- The ddraw fixture gives no diagnostic.
- The table fixture shows the rules: two areas, five valid slots, one dropped entrance, and map 3 kept apart from the malformed header after it.

Before any rule was changed, the audit tool reproduced the engine's 37 diagnostics and 3 dropped entrances from SPEC-018's text alone.

These vectors encode SPEC-024c's own rules. They prove the spec is self-consistent. They do not prove agreement with a running sfall, and they were not run in the engine.

## Regenerating

The tools under `tools/spec024c/` are research-side and must not be copied to the implementation repository. Only this directory and the spec cross the wall.

From the vault root:

```sh
python3 -I tools/spec024c/gen_oracle.py oracle/SPEC-024c
```

`table-measured.tsv` also needs both mods. Give the tool a name, then a `maps.txt` and a `city.txt`, for each corpus:

```sh
python3 -I tools/spec024c/table_audit.py oracle/SPEC-024c/table-measured.tsv "$NAME" "$MAPS" "$CITY"
```
