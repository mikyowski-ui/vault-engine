# Provenance

Vault Engine is built under a strict two-team clean-room process. This document
records where every kind of material in the repository comes from, and how
compatibility is checked without copying protected expression.

## The two teams

| | Research side (`research-vault`) | Implementation side (`vault-engine`, this repository) |
| --- | --- | --- |
| Role | Studies the reference behavior and writes behavioral and binary-layout specifications | Implements the engine from approved specifications only |
| May consult | Reference material, the user's own locally installed game data, research tooling | `specs/approved/SPEC-*.md`, `oracle/SPEC-*/`, and the synthetic fixtures committed here |
| Never hands over | Source code, decompilations, disassembly, research tools, or game assets | — |
| Produces | Approved specs and oracle vectors | Rust code, synthetic fixtures, and tests |

The two sides work in separate repositories with separate agent sessions.
Implementation sessions cannot read the research repository. Communication
across the wall is one-way, through approved hand-off material. When a spec is
incomplete, the implementation side asks for a revision. It never investigates
the reference itself.

## What crosses the wall

Only two kinds of material cross:

1. **Approved specifications** (`specs/approved/`). These are prose
   descriptions of formats, formulas, opcodes, and state machines. Each
   behavior is classified as `[Must-match]` (gameplay-visible behavior and
   binary layouts that must be reproduced exactly) or `[May-improve]`
   (reference memory-safety defects and undefined behavior, which this engine
   replaces with explicit errors or stated safe rules). Specs cite their
   evidence only as reference revision `R1`, never as a repository path or
   commit hash.
2. **Oracle vectors** (`oracle/SPEC-*/`). These are hashes, counts, header
   values, short sample excerpts, and hand-designed synthetic streams. Each
   oracle directory's README describes what it holds. The research-side
   generators that produce them stay behind the wall, and their READMEs say so.

Nothing else crosses: no source listings, no decompiler or disassembler output,
no research scripts, and no original game assets.

## The quarantine barrier

Before hand-off, every spec passes the research vault's `spec_lint.py`
quarantine check. A spec that fails it is quarantined on the research side, not
delivered. On this side, the ingestion rule in `CLAUDE.md` repeats that check.
A builder must reject any incoming spec that contains:

- file paths under a `src/` directory,
- 40-character git commit hashes, or
- raw C decompilation listings.

A rejected spec is not redacted or sanitized on the builder host. The builder
fails fast and asks the research vault for a sanitized revision, so tainted
text never enters this repository's history. Commit `40718af` replaced earlier
reference commit identifiers in the approved specs with `R1` under this rule.

## Hash-only oracle verification

Compatibility with the original data is verified without committing it:

- The oracle tables hold SHA-256 digests (or digest prefixes) of corpus files
  and of decoded outputs: ACM samples, MVE frames, palettes, and PCM. They also
  hold counts and header fields, but no corpus pixels, audio, or text.
- `asset-audit` decodes the user's own locally installed corpus and compares
  each result with the oracle row. A match proves bit-exact behavior. The
  repository never contains the bytes being compared, only their digests.
- `--validate-dat` checks every `MASTER.DAT`/`CRITTER.DAT` member against the
  SHA-256 of the matching loose file in the same way.

## Fixtures and game data

- Everything under `tests/fixtures/` and `oracle/SPEC-*/synthetic/` was written
  for this project. None of it is original game data (see
  `tests/fixtures/README.md`).
- Original game data is supplied by each user from their own installation and
  is never committed. The local corpus path (`/corpus/`) is gitignored. Fuzz
  corpora are generated locally from the synthetic fixtures and are not
  committed (see `fuzz/README.md`).

## Contributor obligations

Every contributor certifies clean-room compliance for each change; see
`CONTRIBUTING.md`.
