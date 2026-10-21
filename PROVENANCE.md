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
quarantine check. The linter fails a spec that contains:

- a reference identifier: an internal symbol name of the reference revision.
  Public SSL opcode and procedure names, and public file-format and
  `fallout.cfg` key literals that happen to coincide with such a symbol, pass
  only through the linter's allowlist; plain English words that match a
  symbol are reported as warnings for human review;
- a file path under a `src/` directory; or
- a raw 40-character git commit hash.

Each approved spec records its linter result in its verification notes: zero
errors, the reviewed warnings, and any allowlisted literals. A spec that fails
the check is quarantined on the research side, not delivered. On this side,
the ingestion rule in `CLAUDE.md` repeats the check. A builder must reject any
incoming spec that contains:

- file paths under a `src/` directory,
- 40-character git commit hashes, or
- raw C decompilation listings.

A rejected spec is not redacted or sanitized on the builder host. The builder
fails fast and asks the research vault for a sanitized revision.

### Recorded exception: commit `40718af`

The repository history does not fully meet this rule:

- Before commit `40718af`, SPEC-001 to SPEC-012 in `specs/approved/` each cited
  the reference revision by its raw 40-character commit hash. The blobs of
  those earlier commits still contain that hash.
- Commit `40718af` replaced the hash with `R1` by editing the specs on the
  builder host. That was a local sanitization, which the ingestion rule added
  to `CLAUDE.md` in the same commit now forbids.
- The history has **not** been rewritten. The affected commits are reachable
  from every branch, including `main` and the `origin` remote.

The current specs carry no hash. Whether to rewrite the history (for example by
replacing those blobs with the research vault's sanitized revisions) is a
decision for the repository owner, and it must be made before the repository is
published. Until then, treat the history before `40718af` as containing tainted
text.

## Hash-only oracle verification

Compatibility with the original data is verified without committing it:

- The oracle tables hold SHA-256 digests (or digest prefixes) of corpus files
  and of decoded outputs: ACM samples, MVE frames, palettes, and PCM. They also
  hold counts and header fields, but no corpus pixels or text. The only decoded
  corpus audio values are short sample excerpts: `oracle/SPEC-010/acm-corpus.tsv`
  lists the first 8 and last 4 decoded samples of each ACM file, and
  `acm-wrap.tsv` the individual samples that wrap.
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
  is never committed. The local corpus path (`/corpus/`) is gitignored, and so
  are the save slots local runs write (`SAVEGAME/`), because a slot holds maps
  and state derived from the corpus. Fuzz corpora are generated locally from
  the synthetic fixtures and are not committed (see `fuzz/README.md`).

## Contributor obligations

Every contributor certifies clean-room compliance for each change; see
`CONTRIBUTING.md`.
