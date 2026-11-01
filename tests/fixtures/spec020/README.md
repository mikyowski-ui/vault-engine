# SPEC-020 Phase L1 fixtures

Synthetic. Team A computed or wrote these from SPEC-020 sections 2.4 to 2.6. They hold
no game data.

- `boot-decision.tsv`: every combination of section 2.4's inputs, and the outcome. A
  `last_played` of `bad` is any other text. With `settings_parse` no, one row stands for
  all, because the launcher opens whatever else is true.
- `switch-cases.tsv`: section 2.5's cases. `world_changed` says whether any world
  state changed, and `no` includes every hash, checkpoint and log.
- `hotkey-hints.tsv`: section 2.6's hints, in order, with their exact text.
