# SPEC-012 Oracle Vectors

Vectors backing `approved/SPEC-012-movies-and-subtitles.md` section 4. These hand-off files hold hashes, counts, header values, and frame numbers, plus two synthetic movies. They contain no corpus pixels, no corpus PCM, and no subtitle text.

| File | Contents |
| --- | --- |
| `mve-summary.tsv` | One row per corpus `.MVE`: size, file SHA-256, frame size, chunk/frame/video-frame counts, timer, audio format, decoded PCM byte count and SHA-256, block-type histogram (types 0-15), opcode histogram |
| `mve-frames.tsv` | One row per show-frame opcode (8978 rows): 1-based frame number, video-decoded flag, display-suppressed flag, palette push range, first 16 hex digits of SHA-256 of the 768-byte device palette, of the displayed 8-bit frame buffer, and (movies with a `.cfg`) of the palette after the fade hook |
| `mve-audio.tsv` | One row per audio opcode delivered to track 0: opcode (8 data, 9 silence), decoded byte count, first 16 hex digits of the SHA-256 of the decoded PCM |
| `moviefx.tsv` | Fade effects parsed from each `art/cuts/*.cfg`, in file order |
| `sve-subtitles.tsv` | One row per parsed subtitle entry, all three languages: frame number, the frame at which it is drawn, text byte length, first 16 hex digits of the SHA-256 of the text bytes |
| `synthetic/synthetic-[AB].mve(.hex)` | Hand-designed 64 × 64 movies covering every block type and subcase, buffer swaps, the suppress marker, track masking, silence, and predictor wraparound |
| `synthetic/synthetic-[AB].expected.tsv` | Per-frame palette/pixel SHA-256, per-audio-opcode SHA-256, total PCM SHA-256 and sample excerpts |
| `synthetic/synthetic-[AB].frames.hex` | Every decoded frame of the synthetic movies as hex rows |

Conventions: PCM is signed 16-bit little-endian. Stereo is interleaved left then right. Silence frames are included in stream order. Palettes are the raw 6-bit table bytes, three per entry. Before a movie starts, the device palette is assumed all zero (black).

Verification: the research oracle's block decoder was compared frame by frame against the reference block decoder, compiled in a scratch harness that was not kept. All 8613 corpus video frames and all 9 synthetic video frames matched byte for byte. The audio delta table was compared value by value with the reference table. The audio decoder itself was checked by reading the reference, not by running it.

The generators under `tools/spec012/` were written on the research side of the clean-room wall. They are **not** hand-off material and must not be copied to the implementation repository. Only this directory and the spec cross the wall.

Regenerate from the vault root, with `C` set to the unpacked corpus and `B` to the oracle binary path:

```sh
gcc -O2 -Wall -o "$B" tools/spec012/mve_oracle.c
python3 -I tools/spec012/survey.py "$B" "$C" oracle/SPEC-012
python3 -I tools/spec012/synth.py oracle/SPEC-012/synthetic "$B"
```

Corpus: `local-corpus/fallout1-unpacked`, as listed in `local-corpus/unpacked-manifest.sha256`.
