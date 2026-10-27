# SPEC-020: Unified Session Runtime and Core UI Authority

**Status:** approved by the owner, 2026-10-27. Revision 1, 2026-10-27, by the coordinator at the owner's request: module paths replace file paths, two module references in the draft are corrected, and tags and a Decisions section are added.

**Provenance:** drafted by the project owner on 2026-10-27 as the contract for Wave 1. Revised the same day by the coordinator. Source categories:

- the owner's draft and brief;
- the project's approved specifications and `AGENTS.md`;
- the names of the workspace's own crates and modules (not their behavior).

No decompilation, disassembly or re-implementation source was consulted, and no game asset was read (`PROVENANCE.md`). This is an architecture specification: it fixes where state and logic live, and it changes no game rule that SPEC-001 to SPEC-019 define.

## 1. Goal & Scope

Consolidate the split execution paths in `vault_platform::game`, `vault_platform::preview` and `vault_platform::map_preview`. Eliminate the duplicated modal state and hit-test logic between `vault_core::ui` and `vault_platform::map_preview`. All changes are clean-room architectural refactors governed by `PROVENANCE.md` and `AGENTS.md`.

**Notation.** Modules are named by their Rust paths: `vault_core::ui::flow` is the `flow` module of the `vault-core` crate. Crate names keep their hyphens in prose. The game edition is `vault_core::GameProfile`, with the variants `Fallout1` and `Fallout2`.

**Compatibility tags.** Every rule carries one tag. In this specification they compare the engine with **its own behavior before the refactor**, not with the original game.

- `[Must-match]`: what a player, a replay or a snapshot can observe stays exactly as it was before the refactor.
- `[May-improve]`: a place where the paths disagree today. The refactor may settle it on the `vault-core` side, but only as section 3.4 allows.
- `[Safe rule]`: how the unified code handles a state or input that one path does not expect. It refuses or clamps; it never panics or reads out of bounds.

## 2. Architectural Invariants

### 2.1 Single UI state and hit-test authority (`vault_core::ui`)

1. These modules own modal state transitions, button hit-testing, and widescreen-aware layout rectangles: `vault_core::ui::flow`, `vault_core::ui::inventory` (inventory, loot, use-on and the barter window), `vault_core::ui::character`, `vault_core::ui::pipboy`, `vault_core::ui::saveload`, `vault_core::ui::options`, `vault_core::ui::called`, `vault_core::ui::talk` and `vault_core::ui::fallout2`. Barter's trade rules stay in `vault_core::barter`; only its window state and hit-testing are in `vault_core::ui::inventory` (Decision D-2). `[Must-match]`
2. The other `vault_core::ui` modules (`automap`, `bar`, `cursor`, `editor`, `endgame` and `sheet`) are under the same authority (Decision D-5). `[Must-match]`
3. Platform adapters in `vault-platform` keep no shadow copies of inventory lists, barter tables, Pip-Boy tabs, or character-sheet edits. They read that state from `vault_core::ui`, and send input to it. `[Must-match]`
4. An adapter that receives an input the current modal state does not accept passes it to `vault_core::ui` unchanged, and acts only on the returned result. It never guesses the outcome locally. `[Safe rule]`

### 2.2 Unified world and viewport stepping

1. Simulation stepping, camera clamping, HUD bar layout, roof X-ray state and touch-bar routing share a single code path for both editions, parameterized by `GameProfile`. The modules are:
   - camera: `vault_platform::map_preview::camera` (Decision D-3);
   - HUD bar layout: `vault_platform::map_preview::hud`;
   - touch-bar routing: `vault_platform::map_preview::touchbar`;
   - the Fallout 1 and Fallout 2 world views: `vault_platform::map_preview::world` and `vault_platform::map_preview::world2` (Decision D-4).

   `[Must-match]`
2. The scripted camera commands and the roam region stay in `vault_core::camera`. The single viewport path reads the roam region from there and does not compute its own. `[Must-match]`
3. Code that differs by edition branches on `GameProfile` in one place. It does not use separate copies of the stepping or layout code. A profile value that a code path does not handle is refused with an error at session start, never handled by a default branch. `[Safe rule]`

### 2.3 Single platform session façade (`vault-platform`)

1. `vault_platform::game`, `vault_platform::preview` and `vault_platform::map_preview` all route through a single `SessionRuntime` pipeline. A session started from the launcher (`vault_platform::launcher`), from command-line flags (`vault_platform` `main`), or from replay tests (`vault_platform::replay`) runs the same simulation, UI and rendering code. `[Must-match]`
2. Each entry point differs only in how it builds the session's configuration and inputs. After that, it runs the same steps in the same order. `[Must-match]`

## 3. Verification Criteria

1. `cargo test --workspace` passes with zero regressions, including `vault_core::ui::tests` and `vault_platform::map_preview::interface_tests`. `[Must-match]`
2. The headless gallery (`vault_platform::gallery`, captured by `cargo xtask capture-gallery`) and the replay logs of `vault_platform::replay` stay bit-identical. `[Must-match]`
3. No existing test is deleted or weakened to make a criterion pass. A test may move only when it moves with the code it tests. `[Must-match]`
4. **Allowed changes in output.** A `[May-improve]` change that alters a snapshot or a replay is allowed only when both of these hold. `[May-improve]`
   - The paths disagree today, and the change keeps the behavior of the `vault-core` side, or the behavior that an approved spec states.
   - The commit names each re-recorded snapshot or log and the rule that justifies it.

   Any other change in output is a regression.

## 4. Decisions

Every point that the draft left open, or that it named wrongly, is resolved here.

- **D-1 (owner instruction).** Scope, the three invariants and the verification criteria are as the owner drafted them, for Wave 1.
- **D-2 (coordinator correction).** The draft listed a `barter` module under `vault_core::ui`, but no such module exists. The barter window's state and hit-testing are in `vault_core::ui::inventory`, which already holds the barter layout. The trade rules are in `vault_core::barter` (SPEC-011 §3.17-3.19). The spec names those two modules. A new `vault_core::ui::barter` module may be split out of `vault_core::ui::inventory` later, if one module still owns the barter window.
- **D-3 (coordinator correction).** The draft's bare `camera` was ambiguous: there are two camera modules.
  - `vault_platform::map_preview::camera` is the presentation camera: view center, zoom and clamping.
  - `vault_core::camera` holds the scripted camera commands and the roam region, and never changes the simulation.

  Invariant 2.2 is about viewport stepping, so it means the platform one. The core one stays where it is, as the source of the roam region (section 2.2, rule 2).
- **D-4 (coordinator default).** The draft's bare `world` names two modules too: `vault_core::world` (the simulation state) and `vault_platform::map_preview::world` (the Fallout 1 world view). Invariant 2.2 pairs it with `world2`, which exists only as `vault_platform::map_preview::world2`, so it means the platform view. `vault_core::world` is not part of this refactor.
- **D-5 (coordinator default).** The draft listed nine `vault_core::ui` modules. The six it left out (`automap`, `bar`, `cursor`, `editor`, `endgame` and `sheet`) come under the same authority, so that no platform shadow state remains for them either.
- **D-6 (coordinator default).** The draft's `gallery/` directory does not exist at the workspace root. The gallery is `vault_platform::gallery`, with its `demos` and `encode` submodules, and its output is whatever `cargo xtask capture-gallery` writes.
- **D-7 (coordinator default).** The edition parameter is the existing `vault_core::GameProfile`, re-exported from `vault-formats`. No new edition type is introduced.

## 5. Verification Checklist

- [x] Module paths only; no file path under a `src/` directory, and no 40-character hash.
- [x] Every rule tagged `[Must-match]`, `[May-improve]` or `[Safe rule]`.
- [x] Every module named here exists in the workspace as of 2026-10-27, except `SessionRuntime`, which this refactor introduces.
- [x] Decisions record each correction to the draft and whether it is an owner instruction or a coordinator default.
