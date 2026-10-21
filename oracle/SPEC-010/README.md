# SPEC-010 Oracle Vectors

Vectors backing `approved/SPEC-010-sound-effects-and-acm.md` section 4. These hand-off files contain hashes, header values, and short sample excerpts: the first 8 and last 4 decoded samples of each corpus ACM file (`acm-corpus.tsv`) and the individual samples that wrap (`acm-wrap.tsv`). They contain no complete corpus audio.

| File | Source |
| --- | --- |
| `acm-corpus.tsv`, `acm-wrap.tsv` | `tools/spec010/survey.py` + `emit_acm_vectors.py` over the local corpus |
| `sfx-resolution.tsv`, `sfx-proto-bytes.tsv` | `tools/spec010/sfx_names.py` over the local corpus |

The generators also write `acm-summary.txt` and the hand-designed streams `synthetic/synthetic-[A-H].acm(.hex)`. Neither was handed over, so this directory does not contain them; the engine tests use synthetic streams A-H written from SPEC-010 §4.1-4.2 (`tests/fixtures/sound/`).

The generators under `tools/spec010/` were written on the research side of the clean-room wall. They are **not** hand-off material and must not be copied to the implementation repository. Only this directory and the spec cross the wall.

Regenerate, from the vault root, with `C` set to the unpacked corpus and `B` to the binary path:

```sh
gcc -O2 -Wall -o "$B" tools/spec010/acm_oracle.c
python3 -I tools/spec010/survey.py "$B" "$C" survey.jsonl
python3 -I tools/spec010/emit_acm_vectors.py "$B" "$C" survey.jsonl oracle/SPEC-010
python3 -I tools/spec010/sfx_names.py "$C" oracle/SPEC-010
python3 -I tools/spec010/synth.py "$B" oracle/SPEC-010/synthetic
```

Corpus: `local-corpus/fallout1-unpacked`, as listed in `local-corpus/unpacked-manifest.sha256`.
