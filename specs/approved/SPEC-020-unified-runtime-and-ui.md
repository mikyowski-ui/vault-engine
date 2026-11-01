# SPEC-020: Unified Session Runtime and Core UI Authority

**Status:** approved by the owner, 2026-10-27. Revision 1, 2026-10-27, by the coordinator at the owner's request: module paths replace file paths, two module references in the draft are corrected, and tags and a Decisions section are added. Revision 2 (Team A, 2026-11-01), on the owner's brief of the same day, adds rule 5 of section 2.1 and the gallery note in section 3. Revision 3 (Team A, 2026-11-01), on the owner's brief of the same day, adds Phase L1: sections 2.4 to 2.6.

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

**Phase L1 (Revision 3).** Sections 2.4 to 2.6 add three pre-multiplayer conveniences: a boot selector, a warm switch between editions, and hotkey toasts. They add new behavior, so in those three sections the tags compare an implementation with **this text**, not with the engine before the refactor. A `[Must-match]` there must be reproduced exactly. SPEC-014's main menu does not change: each of the three happens outside it. `[Must-match]`

## 2. Architectural Invariants

### 2.1 Single UI state and hit-test authority (`vault_core::ui`)

1. These modules own modal state transitions, button hit-testing, and widescreen-aware layout rectangles: `vault_core::ui::flow`, `vault_core::ui::inventory` (inventory, loot, use-on and the barter window), `vault_core::ui::character`, `vault_core::ui::pipboy`, `vault_core::ui::saveload`, `vault_core::ui::options`, `vault_core::ui::called`, `vault_core::ui::talk` and `vault_core::ui::fallout2`. Barter's trade rules stay in `vault_core::barter`; only its window state and hit-testing are in `vault_core::ui::inventory` (Decision D-2). `[Must-match]`
2. The other `vault_core::ui` modules (`automap`, `bar`, `cursor`, `editor`, `endgame` and `sheet`) are under the same authority (Decision D-5). `[Must-match]`
3. Platform adapters in `vault-platform` keep no shadow copies of inventory lists, barter tables, Pip-Boy tabs, or character-sheet edits. They read that state from `vault_core::ui`, and send input to it. `[Must-match]`
4. An adapter that receives an input the current modal state does not accept passes it to `vault_core::ui` unchanged, and acts only on the returned result. It never guesses the outcome locally. `[Safe rule]`
5. The loot window's rectangles, under both editions, are hit-tested in `vault_core::ui::inventory`, from SPEC-019 section 3.7 and SPEC-013 section 3.16. An adapter draws from them and keeps no copy. `[Must-match]`

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

### 2.4 Zero-argument start and the boot selector

**Zero-argument start.** A desktop start with none of `--game-profile`, `--launcher`, `--game`, `--preview-*`, `--play-movie`, `--smoke-test`, `--replay-inputs`, `--master-dat`, `--critter-dat` or `--corpus-dir`. Display flags, such as `--scaling` or `--preset`, do not stop it from being one. There is no `--edition` flag: `--game-profile` is that flag (Decision D-11). `[Must-match]`

**Discovery.** For each of the two editions, the engine looks at its paths in `vault-engine.toml`, then at the existing default install locations, and validates what it finds as it does today: only directory tables are read, and a folder counts for an edition only when its `MASTER.DAT` opens in that edition's archive format. Writing a new `vault-engine.toml` stays as it is. `[Must-match]`

**Mod layers.** The enabled layers of `mod_list` are counted, and the selector shows the count. Discovery lists layers with `find_layers` (SPEC-023 section 3.12) but never enables or mounts one. `[Safe rule]`

**What happens.** `[Must-match]`, in this order; `tests/fixtures/spec020/boot-decision.tsv` holds every combination:

| Condition | Result |
| --- | --- |
| `vault-engine.toml` does not parse | The launcher, as today |
| Neither edition found | The launcher, as today |
| One edition found | That edition boots at once, as today, with no selector and no banner |
| Both found, and `last_played` names one of them | The countdown banner |
| Both found, and `last_played` is absent or anything else | The selector, which waits with no timeout |

**Selector.** A pre-boot screen, not a game screen. It uses the launcher's font and palette and no game art, in the same 640-by-480 frame, so it works before any game data is mounted. `[Must-match]`

- Its title is exactly `Found Fallout 1 and Fallout 2. Pick one.` Over a running session it reads `Switch game. Unsaved progress is lost.` instead. `[Must-match]`
- It has two rows, `1  Fallout 1` and `2  Fallout 2`, each with its enabled-mod count when above 0. A row whose edition was not found is disabled, and says `not found`. `[Must-match]`
- `1` or `2`, or a click on a row, chooses that row at once. Up, Down, Left and Right move the highlight, and Enter chooses it. A pointer over a row highlights it. `[Must-match]`
- The highlight starts on `last_played`, else on the `game` setting, else on Fallout 1. `[Safe rule]`
- Esc, and a closed window, end the process when no session is running. F11, Alt+Enter and Alt+S work as in the launcher. `[Safe rule]`
- Choosing an edition sets `game` and `last_played` in one commit (SPEC-023 section 3.10), then boots it through the same path as the launcher's Play. A failed boot opens the launcher, showing why, as Play does today. `[Must-match]`

**Countdown banner.** A pre-boot screen in the same style. `[Must-match]`

- It reads `Starting Fallout 2 in 3. Press any key to switch.`, with the edition's name and the seconds left, 3, 2 then 1.
- 3000 milliseconds of shell time after its first frame, the edition boots. It writes nothing, because `last_played` already holds that edition.
- A key press or a mouse button stops the countdown and opens the selector. Pointer movement, a lone modifier key and the shell keys F11, Alt+Enter and Alt+S do not.

**Outside the game.** The selector and the banner run before a session exists. They run no simulation, use the wall clock only for the banner, and write nothing to a `--record-inputs` log, which starts when the game does, as it does after the launcher. No replay, smoke test or gallery capture ever shows them. `[Must-match]`

**After the choice.** The session runs SPEC-014 section 3.1, or SPEC-019 section 3.2, from its first step. Nothing is added to the splash, the movies or the main menu. `[Must-match]`

### 2.5 `Alt+L`: the warm switch

**The key.** Alt+L is a shell key, like Alt+S, under both presets and both editions, on every screen of an interactive desktop session. The game never receives it, and it is not logged. It is caught at the next step boundary, so a step always finishes first. `[Must-match]`

**Opening.** It opens the selector of section 2.4 over the session. While the selector is open, no headless step runs, no input record is written, the frame clock stops, and audio pauses. So a session that opens the selector and cancels gives the same log, checkpoint stream and state as one that never opened it. `[Must-match]`

**Closing without a switch.** Esc, Alt+L, or choosing the edition already running closes the selector, and the session resumes at the same step. `[Must-match]`

**Switching.** Choosing the other edition: `[Must-match]`

1. **Validate first.** The chosen edition's install is validated, as discovery does. A failure shows its reason on that row, and the running session stays untouched.
2. **Tear down.** The running `SessionRuntime` ends, as at quit: audio stops, a `--record-inputs` log ends, and pending settings commit. The world, its VM programs, the hook registry, mod sessions, and every cache of decoded assets go with it, and the `Vfs` mount is dropped. Nothing is saved, so unsaved progress is lost, as the selector's title says.
3. **Boot.** A new `SessionRuntime` is built from a zero-argument start's configuration for that edition, with `game` and `last_played` set. Only the window, the renderer's device and the `StorageBackend` carry over, and the OS process does not restart.

**Flags.** Command-line flags do not carry over, except display flags. So a session started with `--sfall` or `--master-dat` does not pass them to the other edition: its settings file and manifests decide. `[Safe rule]`

**A clean boot.** No state crosses the switch: a warm-booted session behaves as a cold start of that edition with the same settings. `[Must-match]`

**Where it does nothing.** A session whose `Vfs` came from memory (SPEC-023 section 3.15) has no discovery, so Alt+L writes one diagnostic. In a replay, a smoke test or a gallery capture there is no shell key. `[Safe rule]`

`tests/fixtures/spec020/switch-cases.tsv` lists each case.

### 2.6 Hotkey toasts

**What they are.** Short hints that teach the controls, one at a time, near the top of the window, in an amber plate like the HUD messages of the enhanced preset. They show under both presets and both editions. `[Must-match]`

| Order | Text | Shown when |
| ---: | --- | --- |
| 1 | `F1: Help` | Always |
| 2 | `I: Inventory` | Always |
| 3 | `S: Skilldex` | Always |
| 4 | `P: Pip-Boy` | Always |
| 5 | `Z: Wait (Pip-Boy clock)` | Always |
| 6 | `F9: Debugger` | Always |
| 7 | `Alt+L: Switch game` | Both editions found |

The texts are exact, and are engine strings, not `.msg` entries. No key's action changes (SPEC-014 section 3.19). `[Must-match]`

**Timing.** `[Safe rule]` for each number.

- The first hint shows 2 seconds after the map screen first appears in a session. Each stays 4 seconds, dissolves over 0.5 seconds as a HUD message does, and is followed by a 1-second gap.
- The timer is shell time, and it stops during combat, dialogue, any window over the map, a movie, and while the F9 panel is open. Hints never show on the main menu or any other screen.
- Pressing the shown hint's key ends that hint at once. A HUD message hides the hint until it has gone.

**The setting.** `hotkey_hints` in `vault-engine.toml` is `once`, the default, `always` or `off`. With `once`, finishing the list sets it to `off`, in one commit. A failed commit is a diagnostic. `[Safe rule]`

**Determinism.** `[Must-match]`

- A toast is shell state. It is never world state, so `WorldState::state_hash`, `combat_hash`, the checkpoint stream, saves, the message monitor and the RNG do not change.
- It is drawn by the shell after the frame is composed, so it is not in the frame that an F12 screenshot or a digest reads.
- It writes no input record.
- It never shows in a replay, a smoke test or a gallery capture, so every replay digest and gallery image stays as it is.

## 3. Verification Criteria

1. `cargo test --workspace` passes with zero regressions, including `vault_core::ui::tests` and `vault_platform::map_preview::interface_tests`. `[Must-match]`
2. The headless gallery (`vault_platform::gallery`, captured by `cargo xtask capture-gallery`) and the replay logs of `vault_platform::replay` stay bit-identical. `[Must-match]`
3. No existing test is deleted or weakened to make a criterion pass. A test may move only when it moves with the code it tests. `[Must-match]`
4. **Allowed changes in output.** A `[May-improve]` change that alters a snapshot or a replay is allowed only when both of these hold. `[May-improve]`
   - The paths disagree today, and the change keeps the behavior of the `vault-core` side, or the behavior that an approved spec states.
   - The commit names each re-recorded snapshot or log and the rule that justifies it.

   Any other change in output is a regression.

   **Gallery authorization (Revision 2).** `cargo xtask capture-gallery` may re-record a gallery image if and only if an existing gallery scene visibly changes because of the Fallout 2 loot layout (SPEC-019 section 3.7) or the cursor layer order (SPEC-014 section 3.27). The commit names each re-recorded file and which of the two causes changed it. Any other gallery change stays a regression. This note covers gallery images only: a replay digest that changes is not authorized here, and goes back to the owner (Decision D-9). `[Must-match]`

5. **Phase L1.** Every row of `tests/fixtures/spec020/boot-decision.tsv` and `switch-cases.tsv`. A warm switch from each edition to the other, then a cold start of it with the same settings and seed, give the same SPEC-021 checkpoint stream. With toasts on and off, a session gives the same `state_hash` values, replay digests and screenshots. Every test here runs headless, by driving the selector's state directly. `[Must-match]`

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

- **D-8 (owner instruction, brief of 2026-11-01).** The gallery authorization of section 3, in its stated limits.
- **D-9 (Team A default).** The note does not extend to replay digests, because the brief names only the gallery. Moving the cursor to the top layer changes every frame where a sprite covered it, so a pinned digest of such a frame needs the owner's decision.
- **D-10 (Team A default).** The loot rectangles are named in section 2.1, because this spec owns where hit-testing lives.

### Phase L1 decisions (Revision 3)

| ID | Decision | Class | Why |
| --- | --- | --- | --- |
| D-11 | The brief's `--edition` is the existing `--game-profile`, and no new flag is added | Team A default | One flag already does this, and two names for it would drift |
| D-12 | Boot selector, 3-second banner, keyboard and mouse choice, and the exact title | Owner instruction (brief of 2026-11-01) | The brief's requirement |
| D-13 | Alt+L opens the selector and warm-boots the chosen edition, tearing down the session and `Vfs` without restarting the process | Owner instruction | The brief's requirement |
| D-14 | Toasts for the seven listed keys, leaving `state_hash` unchanged | Owner instruction | The brief's requirement |
| D-15 | SPEC-014's main menu is not touched | Owner instruction | The brief's requirement |
| D-16 | With one edition found, the start stays as it is today, with no banner | Team A default | There is nothing to switch to, and a delay would only slow every start |
| D-17 | A new `last_played` key records a real choice, separate from `game` | Team A default | First-run discovery writes `game` by itself, so `game` cannot show that a player chose. An existing user with both editions therefore sees the selector once |
| D-18 | Any key or click on the banner opens the selector, Enter included | Team A default | The brief says any key switches. Waiting out the 3 seconds is how to accept |
| D-19 | `1` and `2` boot at once | Team A default | A second key press adds nothing on a two-row screen |
| D-20 | Mod layers are counted and listed, never enabled by discovery | Team A default | Enabling a found mod would change a game the player did not ask to change |
| D-21 | While the selector is open over a session, nothing runs, is logged or is timed | Team A default | Otherwise a cancelled Alt+L would change a recorded log |
| D-22 | There is no autosave and no second prompt. The title warns about unsaved progress | Team A default | The switch already takes two deliberate actions, and an autosave would overwrite a slot nobody chose |
| D-23 | Choosing the running edition resumes it and does not reboot it | Team A default | A reboot would throw away progress for no gain |
| D-24 | The install is validated before teardown | Team A default | A failed switch must not cost the running session |
| D-25 | Only display flags carry over a switch | Team A default | `--sfall` is refused under `fallout1`, and `--master-dat` names one edition's data |
| D-26 | A log ends at a switch, and the new session is not recorded | Team A default | A log holds one session, and replaying across a switch would need a new record kind |
| D-27 | Toasts are a shell overlay, not message-monitor lines | Team A default | The monitor is part of the frame a digest covers, and Vanilla's digests must not move |
| D-28 | `hotkey_hints` defaults to `once`, and has no launcher row or menu entry | Team A default | Hints should teach once and then stop. No SPEC-014 window changes |
| D-29 | The hint order and the timing numbers | Team A default | The brief gives the keys only. The order follows the brief's list |

## 5. Verification Checklist

- [x] Module paths only; no file path under a source directory, and no 40-character hash.
- [x] Every rule tagged `[Must-match]`, `[May-improve]` or `[Safe rule]`.
- [x] Every module named here exists in the workspace as of 2026-10-27, except `SessionRuntime`, which this refactor introduces.
- [x] Decisions record each correction to the draft and whether it is an owner instruction or a coordinator default.
