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

## In-session spec authoring (Team A)

From 2026-10-25 the owner authorized a second route for specifications
(`AGENTS.md`, "Spec Authoring (Team A)"). When no research-vault file is
attached, a dedicated Spec-Author sub-agent (Team A) in the builder's own
session drafts and approves the spec.

- **Its sources:** public format standards, the project's approved specs, the
  owner's briefs, and its general knowledge of publicly documented formats.
- **Never:** decompilations, disassembly, re-implementation source,
  proprietary assets, `src/` paths or 40-character hashes.
- **The implementation:** another agent writes it, from the approved spec
  only.

This is a weaker separation than the two-repository process above, and the
specs it produces say so:

- Team A has no access to the reference, and is told not to consult
  re-implementations. Its general knowledge, however, comes from model
  training data whose own sources cannot be audited, so no Team A spec can
  certify that none of its content traces back to reverse-engineered material.
- Each Team A spec therefore carries a **Provenance** line and a
  **Decisions** section. The **Decisions** section separates owner
  instructions from Team A's own defaults.
- Specs from this route are listed here. A later research-vault revision of
  any of them supersedes it.

| Spec | Route | Notes |
| --- | --- | --- |
| SPEC-015 | Coordinator, from the owner's brief and RFC 1950/1951 | The owner confirmed the open points on 2026-10-25 |
| SPEC-016 | Coordinator, from the owner's layout brief; Revision 2 by the coordinator | Revision 2 corrects ladder records and accepts 412-byte critters, measured on the owner's retail install |
| SPEC-017 | Team A; Revisions 2 and 3 by the coordinator | Every word number is provisional. Fallout 2 script execution stays off until its §8 audit of a real Fallout 2 install is recorded. Revision 2 (owner's request) revises the audit model from the measured Fallout 1 findings, citing counts, one script name and one word, with no script bytes. Revision 3 records the retail Fallout 2 audit (0 findings) and opens the gate; every host effect stays provisional until confirmed in play |
| SPEC-018 | Team A, with the owner's archive-role decision; Revisions 2 and 3 by the coordinator; Revision 4 by Team A | Revision 2 corrects the table grammars against the owner's retail install (its R7-R13). Revision 4 gives the 432-byte Fallout 2 character sheet (R17). The coordinator drafted it with the implementation. Team A then authored it after its own independent re-measurement on the same install, without reading the engine's code. The extra word's position and meaning are recorded as unestablished |
| SPEC-019 | Team A; Revision 2 by the coordinator; Revision 3 by Team A | Layout numbers and gameplay values marked `[Team A default]` are Team A's own choices. Revision 2 records the retail art audit. Revision 3 places the 592-by-260 premade portraits in the monitor window (D-32). The coordinator drafted it with the implementation. Team A then authored it after its own independent re-measurement of the portraits' fit on the same install, without reading the engine's code |

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
