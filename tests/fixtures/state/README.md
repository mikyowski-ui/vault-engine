# SPEC-006 world-state fixtures

- `campaign.gam` and `map.gam` are the complete section 4.1 text vectors. Tests
  exercise CRLF, marker mutations, column-zero comments, 257/258-byte boundaries,
  missing sections, and rejection of undefined numeric initializers.
- `player.gcd.hex` specifies every byte of the 428-byte section 4.2 sheet. Its
  zero ranges are explicit. Tests cover every truncated prefix, invalid names/
  indices, raw fields, player-only normalization, and the HP-base mutation.
- `support.rs` builds independent synthetic programs, readable bitmap/AAF fonts,
  head/background/interface art, and interactive maps. It references the existing
  world/format fixture helpers through one shared module per test binary.

Core tests assert every worked stat/skill/HP-history vector, RNG output and
critical threshold, IQ boundary, head-ID/phoneme mapping, repeated spatial match,
and queue ordering vector from sections 4.3–4.4. Integration tests drive a real
compiled dialogue through a callback and back to the map, then pick up an object.

`cargo xtask fixtures --output <empty-directory>` creates a usable synthetic
corpus. `maps/synthetic.map` has an interactable Guide; `maps/dialogue.map` opens
a no-head conversation in its initial stream for a noninteractive GPU smoke test.
All names, programs, artwork, and dialogue in this corpus were authored for tests.

`write_cam_demo` adds the `enhanced` camera demo to such a corpus.
`cargo xtask fixtures --output DIR` writes the plain corpus to `DIR` and a
second copy with the demo added to the sibling directory `DIR-cam-demo`, so a
Vanilla audit of `DIR` stays clean. The demo is `maps/cam_demo.map` (scroll
blockers on the ring of hex columns and rows 60 and 140, a walled, roofed
building with a doorway, a critter and an item inside, and an entry hex
outside it), an empty `maps/cam_demo.gam`, and `scripts/camdemo.int`, whose
row is appended to `scripts/scripts.lst` (index 2). Its `start` tests
`script_action` for 15 and then makes the four camera requests at once: pan to
the interior over 1500 ms, zoom to 200% over 1000 ms, shake 6 for 800 ms,
reset over 1500 ms. Only an enhanced parse accepts the script; under Vanilla
it fails with the unknown-word error and the map loads without it.
`write_corpus` itself is unchanged.
