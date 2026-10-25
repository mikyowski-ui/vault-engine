# Fallout 2 support and enhanced QoL: roadmap

This document has two parts. The first audits what Vault Engine needs to load
and run retail Fallout 2 beside Fallout 1 in the same binary and launcher. The
second designs the `enhanced` quality-of-life layer that brings some of
Fallout 2's gameplay conveniences to Fallout 1 natively, with no `.DAT`
conversion.

## Clean-room status

Every approved specification (`specs/approved/SPEC-001` to `SPEC-014`)
describes Fallout 1, and three of them rule Fallout 2's formats out:

- SPEC-003 accepts MAP version 19 only and refuses version 20.
- SPEC-008 defines the LZSS archive, and its last paragraph says there is no
  zlib/DEFLATE wrapper.
- SPEC-005 covers the host words `0x804C` to `0x8155` and stops there.

Under the clean-room policy nothing below is implemented from memory or from
other engines. Each Fallout 2 item is a **spec request** for the research
vault. Until those specs are approved, the engine knows the Fallout 2 profile
by name only and refuses it with a message pointing here (see "Groundwork").
The QoL designs in part 2 are original engine extensions built on the
Fallout 1 specs; no Fallout 2 or mod code was consulted.

## Part 1: Fallout 2 gap audit

The phrase "the spec must define" marks what this document does not assume.

### vault-formats

- **Archives (SPEC-015).** The spec must define the second archive version:
  where its directory sits (the user's brief names a footer-located
  directory tree), its entry fields, how members are compressed (the brief
  names DEFLATE), and how a reader tells it from the SPEC-008 version 1
  archive. `DatProvider` and the `Vfs` trait stay; a second archive reader
  joins `DatArchive`, picked by the probe the spec defines. The DEFLATE
  decoder would be a new dependency (`flate2` or `miniz_oxide`), added only
  then, with `cargo deny` checked.
- **Maps and prototypes (SPEC-016).** The spec must define the MAP version
  20 header and object differences from SPEC-003's version 19, and the
  prototype field differences per category. Today `MapFile::parse` refuses
  version 20 by design; it would gain a version-20 path, and the writers a
  matching one, with round-trip tests like SPEC-003's.
- **Unchanged-or-not list.** The spec set must state, per format, whether
  FRM, PAL, LST, FON/AAF, MSG, ACM, MVE, SVE/LIP, GAM, GCD and the save files
  are the same as in Fallout 1 or differ, and how.
- **Fallout 2 data files (SPEC-018).** Whatever text tables the game reads
  (world map, party, and similar), with their grammar.

### vault-vm

- **Host words (SPEC-017).** The spec must define the words `0x8156` to
  `0x8183` the way SPEC-005 defines its range: operands, results,
  diagnostics and the safe rules. It must also state any VM-core
  differences from SPEC-004.
- **Per-game word sets.** The groundwork already makes the accepted word set
  a property of the game profile (`vault_vm::WordSet`). Fallout 1's set is
  exactly SPEC-004/005's; Fallout 2's is "not yet specified" and refuses.
  The enhanced range `0x8200..=0x82FF` stays separate.

### vault-core

- World rules that differ: party control, vehicles, world-map encounters,
  skills, perks, traits, and the campaign tables. Each needs a spec section,
  then a per-profile rule set beside today's Fallout 1 rules, chosen by
  `GameProfile` the way `EnginePreset` chooses Vanilla or Enhanced
  behavior.
- **Saves (SPEC-018).** The save layout per game. Slots stay per game; a
  Fallout 1 slot never loads under the Fallout 2 profile.

### vault-platform

- **Launcher and settings.** The `game` key (`fallout1`, the default, or
  `fallout2`) and its launcher row exist now. The archive members the
  launcher checks are per game; Fallout 2's list comes from SPEC-015/018.
- **Interface (SPEC-019).** The interface art set, layouts, game flow and
  movies per game.

### Spec requests

| Spec | Scope |
|---|---|
| SPEC-015 | Archive version 2: directory, entries, compression, the probe |
| SPEC-016 | MAP version 20 and prototype deltas, with round-trip vectors |
| SPEC-017 | Host words `0x8156`-`0x8183` and VM deltas from SPEC-004 |
| SPEC-018 | Saves, world map, party and the other data tables |
| SPEC-019 | Interface, game-flow and movie deltas |

### Groundwork (done without a spec)

- `game = "fallout1" | "fallout2"` in `vault-engine.toml`, a `--game-profile`
  flag, and a launcher row. `vault_core::GameProfile` names the two games.
- A Fallout 2 launch is refused before any data is read, with the message
  "Fallout 2 data needs SPEC-015 to SPEC-019 (docs/fallout2-and-qol-roadmap.md)".
  The launcher shows the same message on Play.
- `vault_vm::WordSet` replaces the fixed `0x8000..=0x8155` test. Fallout 1's
  set is unchanged word for word.

## Part 2: "Better Than Et Tu" enhanced QoL for Fallout 1

Everything here runs only with `preset = "enhanced"` (from
`vault-engine.toml`, the command line, or a mod's `vault-mod.toml`). Under
Vanilla the menus, windows, saves and replay digests are byte-identical.

### a) Push

- **What.** A **Push** entry in the action menu of a party member or a
  non-hostile critter, out of combat. The critter steps one hex directly away
  from the player; when that hex is blocked, the two neighboring directions
  are tried. With no free hex, a status line says so and nothing moves.
- **How.** The step goes through the ordinary walk sequence (SPEC-007), so it
  is animated, deterministic and replayable. No script procedure is
  invented: Fallout 1 scripts have none for pushing.
- **Art.** Fallout 1 has no push icon, so the engine draws one in the
  40-pixel menu style from the palette.

### b) Take All and the Shift highlight

- **Take All.** The loot window already takes everything on Shift+A
  (SPEC-013, G-20). The enhanced window adds an engine-drawn **Take All**
  button that does the same; it is absent in steal sessions.
- **Shift highlight.** While Shift is held on the enhanced map screen, every
  item lying on the ground is outlined, so loot stands out. It is a separate
  layer after the roofs, outside the SPEC-014 list of 100 outlines, so it
  never changes Vanilla's outlines.

### c) Companion gear and combat distance

- **Gear.** A **Gear** entry in a party member's action menu, out of combat,
  opens the loot window on that companion with no steal roll, no
  `pickup_p_proc` override and no chance of being caught. Equipped items stay
  hidden, as in the loot window.
- **Distance.** The window has a **Distance** toggle cycling *Default*,
  *Close*, *Hold* and *Charge*. It changes how that companion's AI packet
  `max_dist` is read at the two places SPEC-009 uses it: the §3.6 step 4
  pursuit stop and the §3.8 step 1 flee stop.

  | Mode | Pursuit stop | Flee stop |
  |---|---|---|
  | Default | the packet's value | the packet's value |
  | Close | 3 hexes | 3 hexes |
  | Hold | 1 hex | 1 hex |
  | Charge | none | the packet's value |
- **Saves.** The settings live in the world's enhanced state and are saved in
  a sidecar `VAULTENG.TOML` beside the slot's `SAVE.DAT`, only under
  Enhanced. `SAVE.DAT` itself is unchanged.

### d) Additive mod registration

A mod can add to the game without replacing whole files, through
`vault-mod.toml`:

```toml
[[scripts]]          # added to the script list; scripts.lst is untouched
file = "camdemo.int" # under scripts/
index = 4096         # at least the base list's length, unique across mods
local_vars = 0

[[maps]]             # reachable by load_map name and the F9 map jump
file = "cam_demo.map"  # under maps/
number = 100           # 66 or more (SPEC-008's map table is 0-65), unique

[[patches]]          # replaces single records of a fixed-shape table
table = "towns"
file = "data/mymod/towns.patch.txt"  # lines: <record index>|<fields>
```

- **Scripts.** A registered index the base `scripts.lst` lacks resolves to
  the registered file and local count.
- **Maps.** `load_map` matches messages 0-65 of `map.msg` first (SPEC-008
  §3.10 unchanged). An unmatched name is then compared with the registered
  maps' file names, uppercased the same way. Registered numbers also resolve
  back to files, so a save made on a mod map loads again.
- **Patches.** Each table starts from its whole-file override, if a layer has
  one, else the compiled table. Patches replace records by index in load
  order (a later layer wins a record), and the result passes the same shape
  validation as a whole-file override.
- **Conflicts.** Two mods registering the same script index or map number,
  or a map number below 66, stop the launch with both mods named.
- **Gating.** Registrations apply under the same rule as table overrides:
  with the Enhanced preset or a mounted mod layer.
