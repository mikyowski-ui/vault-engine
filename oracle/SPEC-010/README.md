# SPEC-010 Oracle Vectors

Vectors backing `approved/SPEC-010-sound-effects-and-acm.md` section 4. These hand-off files contain hashes, header values, a few sample values, and synthetic streams; they contain no corpus audio.

| File | Source |
| --- | --- |
| `acm-corpus.tsv`, `acm-wrap.tsv`, `acm-summary.txt` | `tools/spec010/survey.py` + `emit_acm_vectors.py` over the local corpus |
| `sfx-resolution.tsv`, `sfx-proto-bytes.tsv` | `tools/spec010/sfx_names.py` over the local corpus |
| `synthetic/synthetic-[A-H].acm(.hex)` | `tools/spec010/synth.py` (hand-designed streams) |

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
