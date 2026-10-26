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

| Spec | Scope | Status |
|---|---|---|
| SPEC-015 | Archive version 2: footer, tree, entries, zlib members | **Approved and implemented** (2026-10-25) |
| SPEC-016 | MAP version 20 and prototype deltas, with round-trip vectors | **Approved and implemented** (2026-10-25) |
| SPEC-017 | Host words `0x8156`-`0x8183` and VM deltas from SPEC-004 | Approved (Team A) and implemented; word numbers provisional; Revision 3 records the retail §8 audit and opens the script gate |
| SPEC-018 | Saves, world map, party and the other data tables | Approved (Team A; §1 owner decision) |
| SPEC-019 | Interface, party control, car and game-flow deltas | Approved (Team A) and implemented; checked against synthetic art only (no Fallout 2 corpus here) |

**SPEC-015** (`specs/approved/SPEC-015-dat2-archive-and-zlib.md`) was written
from the project owner's brief and RFC 1950/1951, with nothing from game
code or other engines. It is implemented as:
- `vault_formats::Dat2Archive`, a read-only VFS provider;
- `vault_formats::inflate`, a bounded RFC 1951/1950 decoder with no new
  dependency.

The launcher reads both games' archives under the Game row.

**SPEC-016** (`specs/approved/SPEC-016-map-v20-and-fallout2-prototypes.md`)
was written from the owner's layout brief:
- version 20 maps have version 19's layout;
- Fallout 2 critter prototypes are 416 bytes, with a trailing `damage_type`.

The game profile and the exact version or length choose the layout. With
SPEC-018 §1's mounting (`vault_formats::Dat2Provider`: loose folder,
`patch000.dat` over `MASTER.DAT`, and `CRITTER.DAT` for critter art),
`--game-profile fallout2 --preview-map` loads and renders a Fallout 2 map.

**SPEC-017 to SPEC-019** are drafted by the Spec-Author sub-agent (Team A)
under `AGENTS.md`'s rule, then implemented by a separate agent.

### Groundwork (done without a spec)

- `game = "fallout1" | "fallout2"` in `vault-engine.toml`, a `--game-profile`
  flag, and a launcher row. `vault_core::GameProfile` names the two games.
- Until SPEC-018, a Fallout 2 launch was refused before any data was read.

**SPEC-018** is implemented: the launcher validates a Fallout 2 install
(`MASTER.DAT`, `CRITTER.DAT` and an optional `patch000.dat` found in its
folder, and the required members), and Play starts a new game on
`maps/artemple.map` from the `data/*.txt` tables (`vault_formats::fallout2`,
`vault_core::fallout2`). Saves use the version 1.2 layout. Fallout 2 scripts
run: SPEC-017 Revision 3 opened the §8.5 gate
(`vault_core::fallout2::SCRIPT_GATE_OPEN`), so every `.INT` loads with the
Fallout 2 word set. A save made while the gate was closed carries the gated
mark and is refused (§7.2). Revision 2's retail corrections are in: the
`worldmap.txt` condition fields, count-less groups, `FIGHTING Player`, item
and scenery encounter types, per-type `Distance`, `Player(Perception)`,
`team_num`, `{Wielded}`, entrances with no marker, the non-critter trunk
record and `level_pids=-1`.
- `vault_vm::WordSet` replaces the fixed `0x8000..=0x8155` test. Fallout 1's
  set is unchanged word for word.

**SPEC-017** is implemented: `WordSet::FALLOUT2` (`0x8000..=0x8183`) with
its own rows (`vault_vm::extensions::FALLOUT2`, `opcode_in`), chosen by the
running game's profile; the six reassigned words, the four words
`0x8156`-`0x8159`, the 42 reserved words that fail closed, `metarule`'s
Fallout 2 rules and `metarule3` 100-111 (`vault_core` `host_fallout2_words`);
`push_p_proc` and the other lifecycle names 24-27; and the §8 audit as
`asset-audit --report-script-stack [--fallout2-dir]`. Under §8's Revision 2
model its §8.4 model check passes on the Fallout 1 corpus, with one recorded
exception (see the README). Revision 3 adds `0x8109` (three operands, I 0,
no effect, one diagnostic per program) and records the retail audit (1248
scripts, 0 findings), which opened the gate. `asset-audit
--report-fallout2-install` repeats the retail checks and a new-game smoke
run.

**SPEC-019** is implemented under the Fallout 2 profile, every change gated
on a Fallout 2 game, so Fallout 1 is unchanged:
- party control (`vault_core::fallout2_party`, `host_fallout2_party`):
  control records, disposition presets and the custom panel's cycling,
  the SPEC-009 deltas at each AI site (flee, healing, target choice, the
  max-distance stop, follow, approach, the snipe retreat, weapon ranking,
  the burst gate), party-member level-ups, dead members leaving with the
  map, hidden entries travelling with the player, and the Push with its
  talk approach and `push_p_proc`. The enhanced Push, Gear and Distance are
  not offered under Fallout 2 (§3.18);
- the car: driving decided at each world-map opening, car steps and fuel
  per travel iteration, running dry, and where the car is;
- the interface (`vault_core::ui::fallout2` and the platform screens): the
  six-button main menu with Options opening the preferences, the bar's
  geometry from `iface.frm` and five indicator boxes, the world map over
  SPEC-018's tile grid with city circles, the town list, the dial and the
  fuel gauge, the town map's entrance markers, the Pip-Boy over
  `quests.txt`, `holodisk.txt`, cities and the 17-movie record, the Karma
  and Kills folders, the dialogue panel's Combat Control button with the
  control and custom panels, trading with party members, Take All, the
  weight line with the carry limit, the old-version save state, the
  `endgame.txt` slideshow, and `elevators.txt` elevators; the custom panel
  is a window the size of its background (Revision 2: at (0, 290) with the
  retail 640-by-190 `custom.frm`);
- `asset-audit --report-fallout2-art --fallout2-dir` lists which §2.2 art
  names an install's `intrface.lst` lacks (D-2).

The tests check the screens for layout, geometry and state with synthetic
art. SPEC-019 Revision 2 records the retail art audit: all 12 §2.2 roles
resolve. With scripts running, the campaign's script effects (areas made
known, party members joining) are exercised by Play; their host effects
stay provisional until confirmed in play (SPEC-017 Decision 38).

### Mod-script verification on load

An engine extension under both profiles. The SPEC-017 §8 stack model lives in
`vault_vm::stack_check`, where `asset-audit --report-script-stack` and the
engine both use it.

- **What is checked.** Each `.INT` that a mod layer serves, through a
  `load_order.toml` layer or a `vault-mod.toml` mod. That covers an override
  of a retail script path and a `[[scripts]]` registration.
  `Vfs::read_with_layer` names the layer that served a read. Retail scripts
  from the base archives or folder are not checked at load: the offline audit
  covers them, `gatedemo.int`'s recorded exception included.
- **How.** The script is decoded with every word accepted. It is run through
  the model with the active profile's table (`fallout1` or `fallout2`, plus
  `0x8200`-`0x82FF` from `vault_vm::extensions::ENHANCED` under the
  `enhanced` preset). It is then parsed under the profile's word set, which
  still refuses words outside it.
- **Clean.** With no finding, the script loads and runs at once. A
  registration needs no retail table row and runs under either profile.
- **Refused.** On a finding or a parse refusal, the script takes SPEC-006
  §3.7's cannot-load path: nothing runs and the host's default follows. The
  engine logs one diagnostic per script, naming the path, the layer, the
  table, and the findings by kind and word. It never logs script bytes.
- **Caching.** The verdict is kept per path for the session, and a clean
  script keeps its checked program. A change of profile or preset checks
  again.

## Part 2: "Better Than Et Tu" enhanced QoL for Fallout 1

Everything here runs only with `preset = "enhanced"` (from
`vault-engine.toml`, the command line, or a mod's `vault-mod.toml`). Under
Vanilla the menus, windows, saves and replay digests are byte-identical.

### a) Push

- **What.** A **Push** entry in a party member's action menu, out of
  combat. Strangers (shopkeepers, guards) are never pushed, except in a
  `--debug` session. The critter steps one hex directly away
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
  a sidecar `VAULTENG.TOML` beside the slot's `SAVE.DAT`, only by an
  Enhanced save that has some to keep. Any other save into the slot removes
  an old sidecar. `SAVE.DAT` itself is unchanged.

### Companion WIELD

- **What.** The Gear window has an engine-drawn **WIELD** button at window
  (214, 184), 108 by 20, above Distance, and a plate at (214, 168), 108 by
  12, naming the held (right-hand) weapon, or *Unarmed*. Each press takes
  the next weapon in hand: the weapons of the companion's inventory in
  inventory order, then none, then the first again. A held item that is
  not among them (a flare) counts as none.
- **How.** A weapon goes through `wield_obj_critter`'s path (SPEC-005: the
  right-hand flag, one unit of a stack); none through `inven_unwield`'s
  (the flag cleared, the entry left in place). The critter's art then takes
  the weapon's animation code (SPEC-007 §2.1) at once, with no animation,
  as the inventory window applies the figure art (SPEC-005 §3.8). The code
  is set only when the critter-art database has the critter's standing art
  with it, through the same FID-to-path catalog; otherwise the art keeps
  weapon code 0, so the weapon is held but drawn unarmed, with no error.
  The window hides the companion's equipment again after each press, so the
  weapon taken in hand leaves the pane and the one put away comes back.
- **Fallout 2.** SPEC-019 §3.18 keeps the Gear window off. Under the
  enhanced preset the same button and plate sit in a party member's Trade
  window (§3.7), along the middle panel's top edge: the plate at window
  (240, 4), 88 by 15, and the button at (332, 4), 66 by 15. §3.7 hides the
  member's held weapon for the session, so a press first puts the hidden
  weapon back at its inventory place, cycles over every weapon, then hides
  the weapon now held, or without one the first weapon, as on entry.
  Leaving the window puts the held weapon back in the right hand. Vanilla
  Fallout 2 trade is unchanged.

### Party status pills

- **What.** The enhanced HUD shows one compact amber-and-black pill per
  active companion: a party member that is a placed, visible critter, not
  dead, other than the player. A non-critter entry such as the Fallout 2
  car trunk has none. Each pill shows the name, `hp/max` and the distance
  mode: *DEF*, *CLS*, *HLD*, *CHG* for the enhanced Distance; under
  Fallout 2 the control record's `distance` (`on_your_own` DEF,
  `stay_close` CLS, `stay` HLD, `charge` CHG, `snipe` SNP, and `---` without
  a control record).
- **Layout.** A fixed stack in the interface frame's top-left corner: pill
  k at (4, 4 + 18k), 152 by 16, at most six (down to y 111). It is clear of
  the bar and indicator boxes (y 358 down) and the debug overlay's panel
  (x 352 right); the windows draw over it, and it shows on the map screen
  only, outside conversations. Colors are the palette's nearest to the
  engine's amber and black keys, as the other engine-drawn buttons use.
- **Clicks.** The first 118 pixels are the name/HP region: under Fallout 1
  it opens the Gear window on the Gear entry's conditions (out of combat, a
  party member); under Fallout 2 it opens the Trade window directly, without
  a conversation, on a standing member with a control record, out of
  combat, and within talking distance (object distance below 9 and an
  unblocked line, else the talk exception's `misc.msg` 2000, with no
  approach). The last 34 pixels are the mode region: under Fallout 1 it
  cycles the enhanced Distance, and under Fallout 2 the control record's
  `distance` takes the next value of the member's allowed set, as a
  SPEC-019 §3.15.3 custom-panel row does, which makes the disposition
  `custom`. In combat, or for a member that cannot take it, the status line
  reads "You can't do that now."
- **Vanilla.** Nothing draws and no click is taken; the pills read the
  host's preset, so the Vanilla frames are unchanged.

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

### e) Auto-open doors

- **What.** Out of combat, the player's walk may pass a door, as SPEC-007
  §3.1 and §3.11.1 already let other critters do. A click beyond the door
  routes through its hex. On reaching it, the player uses the door through
  the ordinary §3.11.4 use, then walks on to the clicked hex. In combat the
  player's routes keep the Vanilla rules.
- **Which doors.** The door must be a closed scenery door with a prototype,
  and its lock word must be neither locked nor jammed. It must also have
  either no script instance, or a script and an earlier open by hand in this
  game. An open by hand is a use from the player's own click that the
  door's `use_p_proc` let through. A script's `use_obj` request does not
  count, and neither does an auto-open. Traps are script-driven in both
  games, so a scripted door the player has never opened stays a blocker.
- **Scripts.** A remembered door's `use_p_proc` still runs on every pass. If
  it overrides, the door stays shut and the walk stops beside it.
- **Saves.** The doors opened by hand are keyed by the map's working `.SAV`
  name, the door's unique object identifier and its PID. They are saved in
  `VAULTENG.TOML`'s `[opened_doors]` table, under the same sidecar rules as
  the companion settings.
