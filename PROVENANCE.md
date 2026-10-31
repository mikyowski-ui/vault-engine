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
| SPEC-017 | Team A; Revisions 2 and 3 by the coordinator; Revision 4 by Team A | Every word number is provisional. Fallout 2 script execution stays off until its §8 audit of a real Fallout 2 install is recorded. Revision 2 (owner's request) revises the audit model from the measured Fallout 1 findings, citing counts, one script name and one word, with no script bytes. Revision 3 records the retail Fallout 2 audit (0 findings) and opens the gate; every host effect stays provisional until confirmed in play. Revision 4 by Team A, from the owner's brief of 2026-10-30, adds the opt-in `FALLOUT2_SFALL` word set that SPEC-024b Decision D-3 asks for. With sfall mode off, no word changes number or meaning |
| SPEC-018 | Team A, with the owner's archive-role decision; Revisions 2 and 3 by the coordinator; Revision 4 by Team A | Revision 2 corrects the table grammars against the owner's retail install (its R7-R13). Revision 4 gives the 432-byte Fallout 2 character sheet (R17). The coordinator drafted it with the implementation. Team A then authored it after its own independent re-measurement on the same install, without reading the engine's code. The extra word's position and meaning are recorded as unestablished |
| SPEC-019 | Team A; Revision 2 by the coordinator; Revision 3 by Team A | Layout numbers and gameplay values marked `[Team A default]` are Team A's own choices. Revision 2 records the retail art audit. Revision 3 places the 592-by-260 premade portraits in the monitor window (D-32). The coordinator drafted it with the implementation. Team A then authored it after its own independent re-measurement of the portraits' fit on the same install, without reading the engine's code |
| SPEC-020 | The owner's draft; Revision 1 by the coordinator | An architecture spec for the Wave 1 refactor: it fixes where UI state and session logic live, and changes no game rule. Its tags compare the engine with its own behavior before the refactor. Revision 1 replaces file paths with module paths and records the module corrections as D-2 to D-7. On 2026-10-30 Team A reworded one checklist line, "a `src/` directory" to "a source directory", so that the repository-wide ingestion grep has no match. That line named no file, and no rule changed |
| SPEC-021 | Team A, from the owner's brief of 2026-10-27 | An engine-side determinism oracle: an FNV-1a 64 state hash and combat sub-hash over a canonical byte stream, and the VM and combat trace formats. It adds no game rule. Its vectors and VM traces were computed by Team A from the spec, and are kept in `tests/fixtures/spec021/`, not in `oracle/`. The seeded combat golden is recorded by the implementer (D-24). Revision 2 by Team A resolves the implementer's open items 4 and 5: W0 now holds its inventory item as entity `3:1`, and a synthetic one-packet `ai.txt` (with a one-line `combatai.msg`) makes the NPC act in the seeded combat trace. Its vectors are in `tests/fixtures/spec021/rev2/`; the superseded revision 1 files stay until the implementer's follow-up switches the tests, which also re-records the seeded golden with the AI loaded. Revision 3 by Team A fixes the implementer's Wave 4 blocker (the NPC's punch could not register its animation sequence, because the synthetic corpus has no punch art for base `sample`): four identical synthetic 75-byte critter FRMs in `tests/fixtures/spec021/rev2/` (punch, hit flinch, fall back, stand-up), written from SPEC-001 and named by SPEC-007, which T11 installs beside the AI fixture. The shared corpus is unchanged. Revision 4 by Team A, from the owner's brief of 2026-10-30, adds a versioned replay checkpoint stream, one row per headless step, and three replay scenarios on the existing synthetic fixtures. The brief called it revision 3, which was taken, so it is numbered 4, and its files are in `tests/fixtures/spec021/rev3/` as the brief names the folder. Team A confirmed that each scenario can run by running the built `vault-platform` binary as a black box on the synthetic corpus, reading only its digest, diagnostics and frames, and read no engine code. Its goldens are recorded by the implementer. A prose correction on 2026-10-30, by Team A on the owner's brief, gives the trip the implementer measured, 138 pixels to X 963, in place of 160 (D-50). The brief said X 983, and the text follows the committed golden. No rule, vector or golden changed |
| SPEC-022 | Team A, from the owner's brief of 2026-10-27 | A GPU-instanced path for the world canvas, with sort keys, batching, a per-hex light buffer and the roof peek parameters. It must match the existing CPU renderer byte for byte, and it adds no game rule. Its vectors were computed by Team A from the spec, using formula tables instead of a palette, and are kept in `tests/fixtures/spec022/`, not in `oracle/`. Where the spec describes the existing renderer (square order, the roof peek), the existing renderer wins and differences are reported for a revision (D-10, D-16). Revision 2 by Team A adopts the Wave 3 implementer's measurements of the existing renderer, recorded as such: its roof X-ray rule, its square order, per-elevation object lists and whole-view refreshes. It gives the enhanced shroud, ground-item highlights and the unlit-scene cutout a slot or mode, and bounds the blend gray level to 0..=7 (palette gray levels never exceed 7). The owner confirmed D-7 and D-11. Its vectors are in `tests/fixtures/spec022/rev2/`; the superseded revision 1 files stay until the implementer's follow-up switches the tests. Revision 3 by Team A, from the owner's brief of 2026-10-30 and the implementer's renderer notes in `docs/REFERENCE.md`, recorded as measured by the implementer, closes the remaining gaps with the reference path and gives a checklist that lifts the four renderer refusals: the numbering of squares and highlights, the shroud's measured rule, every item's mode in an unlit scene, and the peek's tile rule and safe bounds. The brief called it revision 2, which was taken, so it is numbered 3. Its vectors are in `tests/fixtures/spec022/rev3/` |
| SPEC-023 | Team A, from the owner's brief of 2026-10-27 (Wave 4, Part B) | In-memory mounting of version 1 and version 2 archives and loose files from byte buffers in `vault_formats::vfs`, and a portable `StorageBackend` in `vault_platform::storage` for settings, save slots and mod manifests. Its archive rules come from SPEC-008 and SPEC-015 and must match the file-backed readers on the same bytes; on the desktop the engine's files keep their paths and bytes. It adds no game rule. Its tiny synthetic archives and storage scripts were written by Team A and are kept in `tests/fixtures/spec023/`, not in `oracle/`. Revision 2 by Team A, from the owner's brief of 2026-10-30, adds an offline-safe `wasm32-unknown-unknown` compile gate for four crates, with zero new dependencies. The brief asked for the spec to be written, but revision 1 already met its first two items, so it is revised, not replaced. Team A read a dependency listing printed by `cargo metadata` and `cargo tree`, and made one `rustc` measurement, and opened no crate manifest or engine code. The listing shows that `vault-formats` needs `cap-std` behind its `host-fs` feature before the gate can pass. Revision 3 by Team A, from the owner's brief of 2026-10-30, restores the stale `.SAV` cleanup of a slot save. It adopts the implementer's report that revisions 1 and 2 had the earlier behavior wrong, and leaves every file that is not a `.SAV` alone |
| SPEC-024a | Team A, from the owner's brief of 2026-10-31 | One hook interface for sfall's `hs_*.int` scripts and for `.INT` scripts that a `vault-mod.toml` declares: one list of hook points, one chain order, global-script loading (taking up SPEC-024b's D-21), and the reserved word range `0x8300..=0x83FF` with its first ten words. Team A read SPEC-024b and `oracle/SPEC-024/`, and consulted no sfall material, so the boundary it gives `HOOK_ADJUSTFID` comes from SPEC-005 section 3.8, is not checked against sfall, and applies to manifest mods only (D-17). Team A ran `vault-platform --help` and `asset-audit --dump-vm-trace` on synthetic data as black boxes, and read no engine code. Its vectors were computed by Team A from the spec and are kept in `tests/fixtures/spec024a/`, not in `oracle/` |

## Research hand-off: SPEC-024b

`specs/approved/SPEC-024b-sfall-opcodes-arrays-and-hooks.md` and
`oracle/SPEC-024/` came from the research vault on 2026-10-30, by the
two-repository route above and not from Team A.

- **Sources.** The published sfall scripting documentation at tag v4.5.1
  (reference revision `S1`): the opcode list, function reference and notes, hook
  reference, arrays and compiler guides, and public script headers. The research
  side states that it did not read sfall's implementation code, and that nothing
  was checked against a running sfall (the spec's Decision D-1). A `[Must-match]`
  tag there means "as documented", except the sidecar migration rules, which
  carry the tag by owner instruction (D-32).
- **Ingestion check, 2026-10-30.** Neither the spec nor any oracle file contains
  a `src/` path or a 40-character hexadecimal string, and the spec has no code
  listing. The builder side cannot run `spec_lint.py`. The spec's checklist
  records zero errors.
- **Revision 3, ingested 2026-10-30.** It came from the research vault by the same
  route. Its sources are the `S1` documents and headers as bundled with *Fallout
  Et Tu* 2.0.3911, compiled `.int` scripts and script source from Et Tu and from
  *Restoration Project Updated* 2.4.34, and the approved specs. The research side
  states that no script text is reproduced, and that sfall's implementation code
  was still not read. This audit closes Decision D-26, and Revision 3 promotes 25
  behaviors and six hooks to tier P1.
  - **Oracle files.** It changes `README.md`, `sfall-opcodes.tsv`,
    `sfall-hooks.tsv` and `sfall-array-vectors.tsv`, and adds
    `mod-opcode-usage.tsv`, the audit's ranked results in numbers and public
    names. That table is evidence, not a test input.
  - **D-46** supersedes D-T1's operand-class narrowing: the value operands of
    `scan_array` and `set_sfall_arg` are class `A`. D-T1's ruling on `ret_class`
    stands.
  - **D-32** makes the sidecar format revision 2, and a revision 1 sidecar now
    migrates on load instead of being refused (vectors MG1 and MG2).
  - **Check.** The same grep finds nothing, and the diff has no code listing. The
    builder side cannot run `spec_lint.py`, and the spec's checklist records no
    linter run for the Revision 3 text.
- **A linter note the hand-off discloses.** Its README says that eleven public
  names in `sfall-opcodes.tsv` also occur as identifiers in reference revision
  `R1`, so the linter flags them if pointed at that table. They are public names
  from sfall's script headers, and the spec text avoids them.
- **What stays behind the wall.** The oracle's generator stays research-side.
  Implementers work from the spec and the vectors only, and sfall's own source
  stays forbidden by `CLAUDE.md`.
- **Vectors.** They are synthetic, and they encode the spec's own rules. They
  prove the spec is self-consistent, and do not prove agreement with sfall.
- **Amended on this side.** Team A wrote Revision 2 on the owner's brief of
  2026-10-30. It approves Decision D-3 as an owner instruction, amends SPEC-017 to
  match, corrects the list of shadowed enhanced words, defines the manifest key
  and a replay flag, and separates its sidecar delete from SPEC-023's stale `.SAV`
  sweep (D-27 to D-29). No oracle file changed. Revision 3 builds on this text.
- **Prose correction, 2026-10-30.** Team A, on the owner's brief of 2026-10-30 and
  from the implementer's report in `05608f9`, aligned sections 4.1, 5.4, 6.4 and
  6.5 with `sfall-opcodes.tsv` (D-T1). No oracle file changed. Revision 3 merges
  this correction, apart from the two classes D-46 reverses.

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
