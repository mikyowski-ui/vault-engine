# Wave 2 brief: F9 Debugger 2.0 and the autoplay bot

The owner's brief of 2026-11-01 is the authority for this work, and there is no
spec-author step. This note is the contract the concurrent agents share. The
clean-room wall is unchanged: no decompilation, disassembly or re-implementation
source, ever. Rules the bot exercises still come from `specs/approved/`.

## Shape

- `crates/vault-core/src/host_player.rs` holds the player's attack, reload, item
  use and world-map rules. The interface and the bot both call these.
- `crates/vault-core/src/autoplay.rs` holds `BotAction`, the logged form of what
  the bot does, and `AutoplayAgent`, which decides from `&WorldHost` and a
  `BotView`. `WorldHost::execute_bot` carries an action out.
- `crates/vault-platform/src/map_preview/bot.rs` holds `MapPreview::bot_view` and
  `apply_bot`, which finishes screen actions, and `Watch`, Watch Mode's state.
  Headless and Watch Mode both drive `MapPreview`, so they are the same game.
- Every mutation goes through a logged record, `InputCommand::Debug` or
  `InputCommand::Bot`, so a replay is exact. A replay runs no agent.

## Rules

- The agent owns its `ParkMiller`. It never touches `world.rng`.
- Objects in logged records are named by `raw.id`, never by a handle.
- Fast-forward uses `advance_time`, never `clock.set`, and is refused in combat,
  where the queue does not run.
- Refused commands change nothing.
- New `DebugCommand` and `InputCommand` variants are additive: old logs parse.
- `vault-core` stays free of `std::fs`, `std::net`, `std::process`, threads and
  clocks, and takes no new dependency, so it keeps building for wasm32.
- The pinned tables in `tests/fixtures/characterization/` and the digests in
  `vanilla_replays_keep_their_recorded_digests` must not change.

## What the game really does

The bot checks these, not what one might assume:

- There is no Explosives skill. Arming rolls Traps (SPEC-013 section 3.10).
- A Crowbar exists only in scripts, through `UseObject`.
- Lockpick's default does nothing: only scripts change lock bits (section 3.8).
- A door changes only through its `damage_p_proc`. An explosion changes no tiles,
  and its art objects expire.
- Critters within 3 hexes of a blast take damage.
- A fuse's clock stops in combat and dialogue.
- `state_hash` skips the clock, queue, stats, flags, world map and VM, and a load
  rebuilds the entity store and reruns Start and MapEnter. Compare as the audit
  below does, never `hash(world) == hash(loaded)`.

## Parity audit

World, to an in-memory slot image, to load A in a second host, to an image, to
load B. Assert `state_hash(A) == state_hash(B)`. Also diff world against A: entities
by `raw.id`, variables, clock and queue, copying the RNG across after each load,
because it is not saved. Start and MapEnter side effects are allowlisted.

## Failure bundle

A folder of `inputs.jsonl`, `checkpoints.tsv`, a manifest (seed, map, preset,
profile, layers) and `report.txt`. The log is cut at the failing tick.

## Stepping

`run_vm` runs synchronously inside a tick, so a world-dispatched script cannot
suspend. `VmBegin` steps a debugger-launched procedure from a frame boundary, with
the world frozen and saving refused until it ends. Stepping inside a critical
section is invisible to scripts only while nothing else runs, which that freeze
guarantees (SPEC-004 section 3.3). A breakpoint in a world script records the hit
and pauses after the tick.

## Decoding

`0x8200..=0x8281` means different words under enhanced and sfall modes (SPEC-024b
section 3.2), and `0x8300..=0x83FF` is manifest scripting's (SPEC-024a section 7).
The decoder takes the session's mode rather than guessing from the number.

## Watch Mode

Space during Watch Mode takes control and never reaches `step_key`, so it cannot
end a combat turn. It is logged as `InputCommand::Autoplay`, and swallowed until
its key comes up. Speeds run several virtual 16 ms frames per shown frame, each a
logged tick, and leave `GameSpeed` alone.

## Open by the owner's decision

Wave 1's profile and storage work departs from SPEC-020 section 2.4 and decision
D-31, SPEC-023 sections 2.4 and 3.10, and SPEC-024b section 3.1 and decision D-28.
The stack budget and `step budget` finding are not in SPEC-017 section 8.2.

## Fixture

`vault_fixtures::campaign::write_campaign(root, edition)` writes a corpus's
campaign over `state::write_corpus`: doors plain, locked, jammed and scripted, a
container, a friend, a foe, a pistol, ammunition, dynamite, plastic, a drug and
exit grids. Fallout 1 has three maps and Fallout 2 two, ending at the world map. It
has no ladder.
