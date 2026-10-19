# Vault Engine

A Rust clean-room engine workspace. Phase 0 establishes the crate boundaries,
loose-directory asset access, deterministic core primitives, and graphics shell.
Phase 1 (`P1-02` / `P1-03`) adds approved PAL, FRM/FR0–FR5, LST, FON, AAF, and MSG
parsers, format auditing, and an interactive sprite/font/message preview.
Phases 2 and 3 Core (`P2-02`, `P2-03`, `P3-02`) add PRO/MAP parsing, isometric
geometry, the INT loader/core VM, and map preview.
Phase 3 Host and Phase 4 (`P3-03`, `P4-02`, `P4-03`) add the host opcode catalog,
GAM/GCD resources, world state, character statistics, shuffled RNG, and interactive
script-driven dialogue. The SPEC-007 phase adds the complete art-identifier codec,
hex routing, rays/perception, and combat resolution and turns, and closes the
remaining SPEC-001–006 behaviors (see "Spec completion notes" below). The SPEC-008
phase adds DAT archives and LZSS, selected-database lookup, legacy save slots with
the working saved-map set and automap database, world travel, and Pip-Boy status
and rest. The SPEC-009 phase adds `ai.txt` packets, the combat snapshot, threat
selection, the NPC turn (flee, healing, weapon choice, called shots, the bounded
attack loop, reloading), team switching, random retargeting, and taunts. The
SPEC-010 phase adds the exact ACM decoder, effect-name builders with the
prototype sound bytes, the effect lookup and alias chain, speech paths, and
relative effect volume. The SPEC-011 phase adds drugs, inventory rules, barter,
and progression. The SPEC-012 phase adds the exact `.MVE` decoder (container,
block video codec, palettes, DPCM audio), paced playback with movie fades and
`.SVE` subtitles, the game movie table and `play_gmovie`, and desktop movie,
inventory, character, and barter screens. The SPEC-013 phase adds the active
skills, kits, sneak, explosives, poison and radiation events, elevators, exit
grids and the loot/steal window; SPEC-014 adds the game flow (splash, main menu,
character selection and creation), the save/load and options screens, the
interface bar, cursor modes and the endgame. The SPEC-007 revision adds the
punch-to-kick draw and missed-shot bystanders, and the SPEC-001 revision the
shipped 33536-byte palette form. The final SPEC-005/007/009/013/014 revisions
add the single intended-target retaliation rule, center-only explosions with
the scenery `damage_p_proc` notice and the 50-tick retry, scripted explosions,
the automap and inventory windows, rolling HP/AC counters, the white death
fade, the large dialog boxes and the help screen; the desktop now streams DAT
members lazily and can run the vault-debug inspector live (`--debug`, F9).

`AGENTS.md` is the repository's implementation policy. `PROVENANCE.md` records
the two-team clean-room process, and `CONTRIBUTING.md` the clean-room
certification every contribution requires. Format behavior follows
`specs/approved/SPEC-001-pal-frm-lst.md` through
`SPEC-014-game-flow-and-ui.md`.
No original game
data is needed to build, test, or run the synthetic shell and preview fixtures.

## Workspace

| Package | Responsibility |
| --- | --- |
| `vault-formats` | VFS, asset/prototype/map parsers, projection and picking |
| `vault-core` | Authoritative world, host services, stats/RNG, dialogue, save foundations |
| `vault-vm` | INT loader, two-stack core interpreter, explicit host interfaces |
| `vault-guidance` | Pip-Boy holodisk and quest status from campaign globals |
| `vault-render` | Indexed sprite/text/map/dialogue composition and palette rendering |
| `vault-debug` | Headless VM stepper and instruction trace, GVAR/MVAR/LVAR watches, and map object/critter inspectors with tick diffs and text rendering |
| `vault-platform` | Desktop shell, asset preview, input command logging |
| `xtask` | Engineering gate runner |
| `asset-audit` | Loose-corpus counts and format validation |

## Build and verify

Use Rust 1.88 or newer, with Cargo, rustfmt, and Clippy installed. `Cargo.lock`
pins the workspace dependencies. Standard checks need no desktop, GPU, or corpus.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# Run all three gates:
cargo xtask check
```

The dev profile builds `vault-formats` and `vault-core` at `opt-level = 2`, so
debug runs of the desktop decode, decompress, light and route at near-release
speed while the other crates keep full debug builds.

CI also builds a Windows release (`vault-platform.exe`, `asset-audit.exe`),
checks dependencies with `cargo audit` and `cargo deny check` (policy in
`deny.toml`). A scheduled nightly workflow builds the `fuzz/` targets and runs
each briefly on synthetic seeds (see `fuzz/README.md`).

On Linux the desktop shell needs a Wayland or X11 session, the corresponding
window-system libraries (including libxkbcommon), and a Vulkan or OpenGL driver.
Building its default `audio` feature also needs the ALSA development package
(`libasound2-dev` / `alsa-lib-devel`); `--no-default-features` builds it without
audio output. Windows and macOS use winit/wgpu/cpal's native backends.

## Desktop shell

```sh
cargo run -p vault-platform
# Present a frame and exit, failing if setup/presentation fails:
cargo run -p vault-platform -- --smoke-test
```

The window displays a centered 16×16 synthetic sprite containing all 256 indices,
scaled to whole pixels where space permits. An `R8Uint` texture holds indices;
WGSL resolves them through a separate 256-entry RGB332 palette texture. The
sprite retains its aspect ratio on resize. Minimized windows suspend rendering;
lost/outdated surfaces are reconfigured.

Keyboard, pointer, wheel, focus, modifier, resize, and quit observations are
written as JSON Lines to stdout. Command IDs begin at 1 in each session:

```json
{"command_id":1,"kind":"key","physical_key":"KeyA","pressed":true,"repeat":false}
```

Pointer coordinates and resize dimensions are physical pixels; wheel commands
specify `lines` or `pixels`. Physical keys use winit key-code names. Escape or the
window close button exits. Operational diagnostics are structured JSON on stderr,
controlled by `RUST_LOG` (default `info`). `WGPU_BACKEND` can select a graphics
backend for troubleshooting. Input order is sequenced deterministically; actual
OS event delivery is external to the simulation.

A captured log replays headlessly (no window, GPU, or audio device) against the
same preview options (`--game` included), then prints the final state digest as
one JSON line; `--replay-frame frame.ppm` also writes the final frame:

```sh
cargo run -p vault-platform -- --master-dat /path/to/MASTER.DAT --critter-dat /path/to/CRITTER.DAT \
  --preview-map maps/v13ent.map --replay-inputs session.jsonl
# {"commands":8,"elapsed_ms":128,"state_sha256":"…"}
```

Logs have no timestamps, so each command advances a simulated 16 ms frame.
Command identifiers must increase; replay stops at `quit` or an unconsumed
Escape. The digest covers the central save (or its encoding error), every
working saved map, the automap (for a map preview or the game's world), and the
presented frame with its palette.
Replays, like every headless `WorldHost`, complete movies immediately
(`MoviePolicy::CompleteImmediately`), so a script's `playmovie` never waits;
the desktop map preview plays them and waits (`MoviePolicy::WaitUntilFinished`).

## Loose-directory VFS and corpus audit

```sh
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game
# Committed synthetic example:
cargo run -p asset-audit -- --corpus-dir tests/fixtures/loose
# Validate every supported asset and report all failures:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-formats
# Resolve every PRO/MAP art identifier through the SPEC-007 codec:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-art-ids
# Round-trip every PRO/MAP through the SPEC-003 writers:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-writers
# SPEC-008: enter every map, round-trip SAVE.DAT, and write/read a temporary slot:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-saves
# SPEC-009: load combatai.msg and data/ai.txt and resolve every critter's packet:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-ai
# SPEC-010: check every ACM and effect name against the oracle tables:
cargo run --release -p asset-audit -- --corpus-dir /path/to/unpacked/game \
    --validate-acm --validate-sfx --oracle-dir oracle/SPEC-010
# SPEC-011: check the item-prototype invariants and consume every drug:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-items
# SPEC-013: skill, lock, kit, explosive, poison, radiation and loot messages, window
# art sizes and the elevator destinations:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-interactions
# SPEC-014: interface art, intrface.lst positions, cursor hotspots, color keys,
# fonts, menu/save/options texts, splash images, credits, endgame slides, premades:
cargo run -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-ui
# SPEC-012: decode every movie, fade file, and subtitle file against the oracle:
cargo run --release -p asset-audit -- --corpus-dir /path/to/unpacked/game \
    --validate-movies --movie-oracle-dir oracle/SPEC-012
# SPEC-008: stream every MASTER.DAT/CRITTER.DAT member and match its SHA-256 to
# the loose file (archives default to the corpus directory's parent):
cargo run --release -p asset-audit -- --corpus-dir /path/to/unpacked/game --validate-dat \
    [--master-dat /path/to/MASTER.DAT --critter-dat /path/to/CRITTER.DAT]
```

`--validate-movies` requires every corpus `.mve` to have an `mve-summary.tsv`
row and every row to match: file size and SHA-256, frame size, chunk, frame,
and video-frame counts, the timer, the audio format, the decoded PCM byte
count and SHA-256, and the block-type and opcode histograms. Every show-frame
must match its `mve-frames.tsv` row (push range, video and suppress flags, the
device palette, the pixels, and for movies with a fade file the palette after
the fade hook), every track-0 audio opcode its `mve-audio.tsv` row, every
parsed fade effect its `moviefx.tsv` row, and every `.SVE` entry its
`sve-subtitles.tsv` row, including the frame that draws it. A fade file
without a movie of its name (the corpus `movie.cfg` example) is reported and
skipped. On the local corpus all 13 summaries, 8978 frames, 8085 audio
opcodes, 18 fade effects, and 147 subtitle entries (15 files) pass.

`--validate-items` checks the SPEC-011 §4.12 invariants: subtypes and reader
lengths, drug selectors (the -2 form only in slot 0, healing through slot 1),
delayed stages, addiction chances and withdrawal perks, the addiction-table,
alcohol, healing, and money PIDs, armor perks and DR/DT ranges (EMP DR above
100), magazine calibers, minimum strength, pack sizes, and the `perk.msg` name
and description ranges. It reports the counts next to the §4.12 survey, then
has the player consume every drug through the engine with the installed
campaign and messages. On the local corpus, all 18 invariants pass, every survey
count matches (242 items: 9 armor, 77 containers, 14 drugs, 43 weapons, 16
ammunition, 77 misc, 6 keys), and the 14 drugs queue the 18 delayed stages.

`--validate-ai` initializes the combat AI exactly as the host does (a missing
file, a packet without all 25 keys, or an empty table fails), then reports
counts only: packets and token diagnostics, critter prototypes whose default
packet resolves or falls back to entry 0, and nonempty taunt ranges with and
without `combatai.msg` lines.

`--validate-acm` requires every corpus `.acm` to have an `acm-corpus.tsv` row
and every row to match: source SHA-256 and size, header values, block count,
zero bytes read past EOF, the SHA-256 of the decoded s16le PCM, and the first 8
and last 4 samples. Every wrapped sample must equal its `acm-wrap.tsv` row.
`--validate-sfx` rebuilds each `sfx-resolution.tsv` request with the builders
(critter-list stems and suffixes, weapon sound bytes), runs the lookup chain
against `sound/sfx/`, and compares the attempt order and resolved stem. It also
recounts the item (0x38), scenery (0x28), and weapon (0x79) sound bytes of every
prototype against `sfx-proto-bytes.tsv`, including which open/close names exist.
On the local corpus, all 2511 ACM rows (124 wrapped samples) and all 760
resolution rows pass.

`--validate-saves` checks that the campaign initializer covers every fixed
SPEC-008 campaign index (towns, entrances, quests, disks, encounter globals),
enters each map through the world host so it joins the working saved-map set,
requires every working `.SAV` to reparse with the saved bit, requires the central
`SAVE.DAT` to be a byte-exact parse/write fixpoint, and writes then restores a
save slot in a temporary directory.

`--validate-art-ids` reports, per source, identifiers whose decoded file exists,
decodable identifiers whose file is absent from the mounted corpus (`MISSING`
lines; not fatal, since the reference guarantees no fallback resource), and
identifiers the codec rejects (`FAIL`; nonzero exit). `--validate-writers` requires
PRO output to equal the consumed input exactly (the safe-rule writer keeps reader
order and omits the reference's scenery suffix), and MAP output to be a parse/write
fixpoint that keeps every saved object and retained script.

The CLI prints total mounted files and counts for `.frm`, `.pal`, `.fon`, `.aaf`,
`.pro`, `.map`, `.int`, `.msg`, and `.lst`, including zero counts. It traverses the
VFS namespace without parsing file contents. Invalid arguments or mounts exit
unsuccessfully with a diagnostic. With `--validate-formats`, the tool additionally
parses every `.pal`, `.frm`, `.fr0`–`.fr5`, `.lst`, `.fon`, `.aaf`, `.msg`,
`.pro`, `.map`, `.int`, `.gam`, and `.gcd`,
prints per-format pass/fail counts, and reports failing paths, formats, byte
offsets, and reasons on stderr. Any failure produces a nonzero exit after the
remaining assets have been checked. Phase 0's `loose/` markers are deliberately
not valid binary formats; use `tests/fixtures/formats/materialize.py` for a valid
synthetic corpus.

MAP validation resolves item/scenery state trailers through the mounted
`proto/<category>/<category>.lst` and `.pro` files. Missing dependencies are
reported as map validation failures. INT validation parses images without
executing game extensions.

Mount the directory whose immediate children include `art`, `data`, `maps`,
`premade`, `proto`, `scripts`, `sound`, `text`, root `color.pal`, and font files
such as `font0.aaf` and `font5.fon`. Folder names and extensions are not an
allowlist: partial corpora and other regular files are supported.

```rust
use vault_formats::{LocalDirProvider, Vfs};

fn main() -> Result<(), vault_formats::VfsError> {
    let assets = LocalDirProvider::mount("tests/fixtures/loose")?;
    let palette = assets.read("CoLoR.PaL")?;
    let sprite = assets.read(r"ART\ITEMS\test.frm")?;
    for path in assets.files() {
        println!("{path}");
    }
    Ok(())
}
```

Paths are relative ASCII names, normalized to lowercase with `/` separators.
Both slash styles are accepted in queries. Absolute paths, drive/UNC paths,
`.`/`..`, repeated/trailing separators, control characters, Windows special
characters/device names, and components ending in dots or spaces are rejected.
Unicode names are rejected to keep case folding identical across host platforms.

Mounts reject symlinks inside the tree, special files, and case collisions in
either files or directories. Reads check for replacement symlinks and use
`cap-std` directory capabilities, so confinement holds even across filesystem
races and renames of the mount's ambient path. A mount snapshots names and lists
them in lexical order; remount to list added or renamed files. Reads use current
contents, and a read that misses the snapshot resolves the path against the live
directory (case-insensitively, with the same symlink and collision checks), so a
stale index never hides a file added later (SPEC-008 §3.2 safe rule). Stable file
contents require an unchanged corpus.

Keep local proprietary data outside the repository or in ignored `corpus/`.

## Sprite, font, and message preview

```sh
cargo run -p vault-platform -- --corpus-dir /path/to/unpacked/game \
  --preview-frm art/items/example.frm
# Explicit overlay selection, also compatible with --smoke-test:
cargo run -p vault-platform -- --corpus-dir /path/to/unpacked/game \
  --preview-frm art/items/example.fr3 --font font0.fon \
  --msg text/english/game/example.msg --message-id 100
```

Left/right arrows wrap orientations 0–5. Up/down arrows step forward/backward
through animation frames, with wrapping. The window title shows the current
orientation/frame. Directional suffixes select the initial orientation while
retaining all six header slots. Escape exits. Preview images use nearest-neighbor
scaling and rectangle centering, which intentionally differs from world-anchor
placement. Sample text and the selected message are composited through the loaded
font and palette; index zero is transparent in sprite drawing.

The viewer loads root `color.pal`. It defaults to `font0.aaf`, then `font0.fon`,
then another root AAF/FON in lexical order (AAF preferred). It selects the first
MSG below `text/` in VFS order and its lowest ID with nonempty text. Explicit
missing/invalid assets or message IDs produce an error. Text remains raw bytes
mapped to font glyphs; the viewer does not assume UTF-8 or change languages.

## Desktop game shell (SPEC-008 to SPEC-012 integration)

```sh
# Loose files, or the SPEC-008 databases with an optional loose patch root:
cargo run -p vault-platform -- --corpus-dir /path/to/unpacked/game --preview-map maps/v13ent.map
cargo run -p vault-platform -- --master-dat /path/to/MASTER.DAT --critter-dat /path/to/CRITTER.DAT \
  [--corpus-dir /path/to/patches] --preview-map maps/v13ent.map
```

`--master-dat` and `--critter-dat` mount a `DatProvider`: ordinary requests use
the master database, and `art/critters/` requests use the critter database. Each
consults the loose `--corpus-dir` first, with no fallback to the other archive.
Every preview and the world host read through this namespace. Archives are
opened lazily (`DatArchive::open`): startup reads only the directory tables,
and each member is read and decoded from the file when requested, keeping the
64 MiB bound per extracted file. `VfsWorldAssets` caches every decoded
resource by path or identifier, failures included (prototypes, list files,
message catalogs, frames and palettes), so a member is read and decompressed
once per session; the frame cache starts over past 256 MiB, and a
conversation's end releases only talking heads and backgrounds. The map view
binds sprites through that cache only for art it has not seen, keeps the
pixel-to-hex light mapping until the view moves, and shades in bulk through
the palette's intensity table.

On the local map, **W** opens the world map and **P** the Pip-Boy; script
`world_map`/`town_map` requests open them too. Both are modal, so the local map's
scripts and clock wait. The world map is the 450 x 442 viewport over the
1400 x 1500 world. The specifications name no world artwork, so the view is drawn
from the fixed terrain grid and discovery values (unknown black, seen dim,
visited bright). It shows known towns (with the specification's town names), the
traveller, and the destination. A click travels to a world pixel, or to a known
town's jittered center. Travel runs at most two loop iterations per update, about
24 updates per second, with time charges, discovery, healing, and encounters.
Arriving in a town enters it: a first visit loads its slot-zero map, later visits
show the town map. **Enter** enters the current town or drops out into the
terrain here. Arrows scroll, and **Escape** returns. The town map draws interface
art `156 + town` at (20,20) with its known entrance buttons, labelled from
`worldmap.msg`.

The Pip-Boy shows quests by location (active green, completed dimmed and struck
through) and the holodisk list and pages, read from `pipboy.msg`. Its alarm clock
offers the SPEC-008 rest choices. Rest advances one UI step per 50 ms through
`WorldHost::begin_rest`/`advance_rest`, so the window stays responsive. Escape
cancels it, and a queue interruption closes the Pip-Boy. SPEC-008 names neither
catalog; these two hold the specified message ranges.

Audio plays SPEC-010 sounds on the default device (`--no-audio` keeps it silent):
- Effect requests resolve through the lookup chain, with the owner's gender
  alias. They are decoded with the exact ACM decoder, cached, and played mono at
  22050 Hz at the relative volume of their owner, at the full effect setting.
- At most four effects play at once. A fifth request fails rather than evicting
  one.
- Talking-head speech plays `sound/speech/<head>/<stem>.ACM`. Its completion
  ends the head's speech phase; without a file or device it ends at once.
- Library sound slots play their files, loop with mode bit 1, and stop when the
  slot stops, pauses, or is freed.

The engine requests these effects:
- `play_sfx` (no owner) and registered animation cues (owner).
- Door open/close and container open on use, including doors that movement opens
  automatically.
- Weapon attack effects, and hit effects with the target's material. Unarmed
  attacks use the punch/kick character effect, with contact on a hit.
- The walk/run character effect when a movement starts.
- The AI's ready and out-of-ammunition sounds.
- Desktop buttons play the small-button press and release names.

Movies (SPEC-012) play in the 640 x 480 window:

```sh
# One movie with its fade file (and subtitles with --subtitles), then exit:
cargo run -p vault-platform -- --corpus-dir /path/to/unpacked/game \
  --play-movie art/cuts/intro.mve --subtitles [--language french]
# The startup movies (iplogo, then intro) before the map:
cargo run -p vault-platform -- --corpus-dir /path/to/unpacked/game \
  --preview-map maps/v13ent.map --startup-movies
```

- In the world, `play_gmovie`, the endgame movie, the water countdown (with a
  host water global), and the Pip-Boy's **Video archive** tab request game
  movies. They queue and play one at a time, modally: the world and its
  scripts wait. Afterwards the previous screen returns.
- One movie step runs per shell update. Sync points wait on the frame timer,
  late frames without a palette push are dropped, and the movie's device
  palette replaces the display palette (color cycling stops). Audio streams
  through the mixer from the first sync point at 0.94 x the full music volume.
- Any key skips a movie. So does a mouse button pressed during the movie,
  once every button is released. Holding a button does not skip.
- Subtitles use font 101 and `art/cuts/subtitle.pal` with the RGB555 white
  text color. They are always on for the boil movies; `--subtitles` turns them
  on for the others.

On the local map, **I** opens the inventory and **C** the character screen
(SPEC-011). Both are modal:
- The inventory is the SPEC-014 §3.34 window at (80, 0): six list rows, the
  two hands, the armor slot and the player figure, which turns one facing
  every 166 ms (a provisional rate), drags included. In the hand cursor a left press on a filled
  slot lifts the item and the release drops it on the first target in the
  §3.34 order (the list, where ammunition dropped on a weapon reloads it
  through the counter for a stack; the left hand; the right hand; the armor;
  nowhere). A right press switches to the arrow cursor: a click shows the
  examine text, a held press opens the item menu (Look, Unload or Use, Drop,
  Cancel). Using a drug consumes it (SPEC-011 §3.6), a kit heals, an
  explosive opens Set Timer and a Motion Sensor the automap. Use on a
  container opens it, up to nine levels deep: the list shows its contents and
  its name, drops into the list go into it, an item dropped on the figure
  moves up one level, and a press on the figure goes up one level. A weapon
  reloaded in a hand stays in that hand. Opening it in combat costs the §3.11
  action points. No approved spec gives the window's
  background, stats panel, scroll and Done buttons, or the Unload and Drop
  icon art, so those are provisional.
- The character screen reconciles levels on opening. It shows SPECIAL, derived
  statistics, skills (tagged ones starred), traits, and perks, and clicking a
  skill spends an unspent point. When a perk is pending, the chooser lists the
  eligible perks by name. Tag! asks for a fourth tag skill and Mutate! for a
  trait to drop and one to take; cancelling either removes the rank. Done
  keeps the sheet, while Cancel or Escape restores the sheet saved on opening.
- A dialogue barter request (`gdialog_barter`) opens the SPEC-013 §3.18
  barter window (`barter.frm` under the conversation) with the dialogue's
  target. Its subwindow shows three rows each of your items, your offer,
  their offer, and their items, with scroll buttons (arrows, PageUp/PageDown,
  Ctrl for the merchant side) and the two totals. Drag an entry onto its
  route's drop rectangle to move it, through the move-items counter for a
  stack. **Offer** (**M**) tries the trade at the §3.18 price; **Talk** (**T**)
  or Escape returns the tables and the hidden equipment, then resumes the
  dialogue (closing a session that already ended).

## Game flow and interface (SPEC-013, SPEC-014)

```sh
cargo run -p vault-platform -- --master-dat /path/to/MASTER.DAT --critter-dat /path/to/CRITTER.DAT \
  --game [--save-dir /path/to/saves] [--config /path/to/fallout.cfg]
```

`--game` runs the SPEC-014 flow: the splash (`art/splash/splash<n>.rix`, chosen
from the `splash` key), `iplogo` and `intro`, then the main menu (Intro, New Game,
Load Game, Credits, Exit; Shift with Credits shows the quotes; two idle minutes
replay the intro). New Game opens the premade selection (**T** take, **M** modify,
**C** create, arrows cycle, **Escape** back); Modify and Create open the editor in
creation mode (SPECIAL plus/minus, tag and trait buttons, **N** name, **A**/**-**
age, **S** sex, **Enter** done with the specification's validation). Taking a
character plays `ovrintro` and starts `maps/v13ent.map` at 07:21, 5 December 2161.
A dead player sees the death scene; `endgame_slideshow` shows the slides chosen by
the campaign globals, and the ending plays `walkm` or `walkw` and the credits.
Quitting returns to the menu. Outside the world, palette fades are presented
as instant changes; in the world they run for 700 ms (SPEC-014 §3.2 safe rule):
the library `fadein`/`fadeout`, `gfade_in`/`gfade_out`, First Aid and Doctor,
and the movie flags (playback ends on black; flag `0x02` fades back). A
session ends on a fade to white; the death scene then fades from white into
`death.pal`, to black when it is dismissed, and the menu fades in. Knockout
and death are checked between world passes, and ending combat wakes a
knocked-out player. **F1** shows `helpscrn.frm` under `helpscrn.pal` (no fade,
cycling suspended) until any key or click.

In the world, the interface bar shows the message monitor, HP/AC counters, AP
pips in combat, the active item slot and the indicator boxes. When the core
reports a change (damage, healing, drugs, a turn's start or end), a counter
rolls one unit per step every `trunc(250 / (|P - V| + 1))` ms, switching color
at 25% and 50% of maximum HP; the world and input wait while it rolls. Its buttons and the
game-loop keys follow §3.19: **I** inventory, **Escape**/**O** options, **S** the
Skilldex, **C** character, **P** Pip-Boy, **B** swap hands, **M** or a right-click
cycle the cursor mode, **A** start combat, **1**-**8** skills (Sneak first), **,**
and **.** rotate, **/** the date, **Ctrl+S**/**F4** save, **Ctrl+L**/**F5** load,
**F6**/**F7** quick save/load, **Ctrl+P** pause, **Ctrl+Q**/**F10** quit. **Space**
ends a combat turn. The Skilldex sets a skill's targeting mode; a click on an
object makes the SPEC-013 skill-on request. Using a container or an untalkable
critter, and the Steal default, open the loot window (Shift+**A** take all, arrows
scroll, **PageUp**/**PageDown** cycle stacked bodies); a drag lifts an entry
on the press and drops it on the release over the other pane, with the
move-items counter for stacks. `metarule` 15 opens the elevator panel. The
options menu leads to the save/load screens (slots below `--save-dir`), the
preferences (written to `--config`) and the quit confirmation.

The world draws its own cursor (§3.26-3.29). Resting the pointer 250 ms shows
the hover: the movement cost (or a red X), the arrow's action icon with the
object's look-at text in the monitor, or the crosshair's hit chance. The
movement hover plans a path only when the hovered hex, the player's hex, AP,
legs or combat state, or a route blocker changes. With item highlight on, an
item under the arrow gets a one-pixel outline in key 30632 until the pointer
moves (no approved spec gives the outline's geometry). A dead critter's
look-at line (491 or 492) draws from a presentation-only generator, so
looking never moves the world's random sequence. Map clicks
act on the release; holding the button 250 ms in arrow mode opens the action
menu (move vertically to choose, release to run it). A pointer on the frame's
edge scrolls the map, and the arrow keys share its blocked test and 33 ms
gate (the blocked art shows where the view would leave the hex grid or the
scroll limits). Modal dialogs are `lgdialog.frm` boxes (302 by 127) at the
§3.13 positions: save/load errors at (169, 116), the Pip-Boy refusal in combat
at (192, 116).

**Tab** or the bar's MAP button opens the SPEC-014 §3.33 automap window
(`automap.frm` at (75, 0), the world paused): seen walls, the exit-grid
marker, the player and, in high detail, seen scenery, each map hex drawn at
pixel `(449 - 2c, 8 + 2r)`. **H** and **L** set high and low detail (the saved
display flag) and the switch toggles it; **S** or Scanner turns on the motion
sensor, which needs a Motion Sensor (PID 59) with charges and adds every
living, visible critter; Escape, Tab, **A** or Cancel close it, and **F10**,
**Ctrl+Q** or **Alt+X** ask to quit. Objects become
seen as the player moves (the visited-hex window around each step), and saving
records the seen state first; the Pip-Boy's automap page draws the stored
raster.

`--debug` (or **F9** during a run) attaches the vault-debug inspector to the
interactive world: every GVAR, MVAR and LVAR is watched after each update, the
live trace records every script instruction, and an overlay on the right of
the map view shows the newest changes (also logged at debug level, target
`vault_debug`).

## Map preview

```sh
cargo xtask fixtures --output target/state-corpus
cargo run -p asset-audit -- --corpus-dir target/state-corpus --validate-formats
cargo run -p vault-platform -- --corpus-dir target/state-corpus \
  --preview-map maps/synthetic.map
# The second fixture opens a no-head dialogue during startup for GPU verification:
cargo run -p vault-platform -- --corpus-dir target/state-corpus \
  --preview-map maps/dialogue.map --smoke-test
```

The fixture output directory must be empty. `--preview-map` and `--preview-frm`
are mutually exclusive. Arrow keys pan, **R** toggles roofs, and **PageUp** and
**PageDown** change the viewed elevation. Hover records on stdout include hex, floor-square, roof-square,
and elevation coordinates; picking reverses the displayed image's scale and
letterbox offset before applying the specification's inverse projection.

A left click acts on its release by the SPEC-014 cursor mode (see "Game flow and interface"):
movement walks, the arrow performs the object's primary action (pick up, talk,
loot, use or look) and selects it, the crosshair attacks; a right-click cycles
the mode. The **L/D/U/G/T** keys route look, description, use, pickup ("get") and
talk on the selected object through the authoritative world and object scripts. Approaches move one hex per animation
step and run spatial checks. Pickup updates inventory and removes the world
representation only after insertion succeeds.

Dialogue uses the actual script reply/options and callback procedure indices.
Click an option or use **1–9**; **0** selects the documented dummy no-callback
choice. **Space/up/down** navigate reply pages, the wheel scrolls options, and
**Escape** closes dialogue (on the map it opens the options menu). Head animation continues
while the world background clock is suspended. The synthetic Guide demonstrates
two nodes with a real VM callback.

`--player-gcd <VFS_PATH>`, `--language <directory>`, and `--seed <integer>` select
the player sheet, language, and deterministic RNG initialization. Defaults are
a neutral character, English, and seed 1. Seed -1 requires explicit host entropy
and is not silently derived from a wall clock.

The map view uses a 640×380 logical viewport, framed with the SPEC-014
indicator boxes and interface bar into a 640×480 image. Floor/roof images use the exact
square projection; roofs are 96 logical pixels above their floor. Objects use
hex centers, stored displacement, directional FRM shifts, and bottom-center
alignment. Tile images are not resized to the diamond footprint. Hidden objects,
hidden/blank tiles, and inventory children are excluded from placed-object drawing.
Flat objects precede upright objects; remaining object order is deterministic by
anchor position and ID. The complete indexed scene scales with the window.

Object and tile art resolves through the SPEC-007 codec (`ArtCatalog`): the FID's
own category selects the list, critters append the two-letter animation/weapon
suffix and the selector's `.frm`/`.fr0`–`.fr6` extension, heads append their
selector/variant, and other categories use the list name verbatim. A decodable
object FID whose file is absent is logged and not drawn; codec failures are errors.
If the requested frame is unavailable, frame zero is reported and displayed.
Cached map art is limited to 256 MiB. Floor index 1 is drawn like any floor (only
roof index 1 is exempt). Palette cycling runs in the shell and follows the world's
cycling state (for example, disabled during dialogue); script-loaded palettes
replace the display palette.

## Format safety and compatibility

All parsers forbid unsafe code and accept at most 64 MiB per asset. Audit/preview
reads enforce this limit before allocation and during reads. Font metrics are
limited to 4096 pixels, FON directories to 256 byte codes, and text tables to
1,000,000 entries. Diagnostics include the format and byte offset. Tests exercise
all section-4 fixtures from the approved specifications, prescribed mutations,
required-data truncation, and deterministic mutation/random-byte fuzz cases.

- `Palette` accepts the complete 33,536-byte RGB-plus-RGB555 base layout without
  a discriminator, as well as an optional four-byte tag/padding. A `CWEN` tag
  requires all three extended tables; a partial tag or table is rejected.
  Valid VGA channels multiply by four
  (63 becomes 252); invalid triples become unavailable black entries. Supplied
  tables are retained, and generated intensity is evaluated lazily. `PaletteCycler`
  implements the specified shared timers, rotations, red pulse, and reset rules.
- `FrmArchive` accepts arbitrary version words and preserves signed timing.
  Adjacent equal direction offsets share records. The SPEC-001 §3.3 safe rule
  requires packed sequential blocks (direction 0 at offset zero), a positive frame
  count, nonnegative dimensions and payload sizes, and payloads of at least
  width x height bytes; the next record starts after the declared payload. The
  number of stored sequences comes from the shared offset slots; the header
  always has six slots, with no separate orientation-count field. The declared
  total area is metadata only and is not checked (split animations retain the
  unsplit total); `stored_frame_area_len()` reports the actual bytes read.
  Trailing bytes after the frame area are ignored. World rectangles, cursor
  hotspots, preview centering, and cumulative displacement are separate operations.
- `LstTable` requires a consumer (`Art`, `Script`, `Prototype`, or `Raw`). Bounded
  line reads, blanks, comment-like lines, and duplicate names retain positions.
  Art/script/prototype lookups apply their distinct ID masks/bases. Unknown
  paths such as `data/*.lst` use raw physical lines without invented name rules.
  Embedded NUL and overflowing numeric metadata are rejected.
- `Font` retains bit-packed FON data and byte-level AAF coverage. It validates all
  drawable extents against the correct loaded block, rejects negative metrics
  and undefined AAF levels 14+, and preserves spacing, bottom alignment, space
  width, complete-advance clipping, and the legacy maximum-cell queries. There is
  no pair kerning. Rendering supports proportional/monospaced text, underline,
  shadow, and the specified palette blend levels, including 8–13.
- `MsgCatalog` preserves byte encodings, concatenates multiline fields, honors
  duplicate replacement, and supports merging. Default loading trims ASCII
  whitespace around signed decimal IDs and skips empty/invalid IDs, extra fields,
  and other malformed input while retaining earlier complete messages. Recovery
  resumes at an indented decimal ID followed by another opening field on the same
  physical line, so a broken record does not consume the following valid record.
  `warnings()` retains the first 128 diagnostics and `warning_count()` counts all
  recoveries. Fields remain capped at 1023 retained bytes, IDs at signed 32-bit
  range, and file/record resource-limit violations remain errors.
  `parse_strict`/`load_strict` retain the original SPEC-002 validation rules for
  authoring and regression tests. `parse` uses archive/raw Ctrl-Z semantics;
  `parse_with_mode(..., TextMode::WindowsLoose)` exposes the original Windows
  loose-stream EOF behavior. Both translate CRLF. Audit/preview consistently use
  raw VFS byte semantics on every host.

Base PAL acceptance, split-animation area metadata, and tolerant MSG recovery
implement the requested real-corpus compatibility corrections to the initial
strict interpretation of the approved specifications. Automated regressions use
synthetic fixtures; original corpus data is not included in tests.

The preview assumes same-coordinate destination sampling for AAF blends, as in
the approved synthetic fixture. The specification leaves original-executable
sampling order unresolved; this implementation does not claim to resolve it.

## Prototypes, maps, and geometry

`Prototype` dispatches all six categories from the PID's full high byte. Items
and scenery have unaligned subtype payloads, and tile/miscellaneous prefixes have
their own lengths. Every known subtype is typed; unknown subtypes are rejected.
`consumed_len` excludes tolerated trailing bytes, including the scenery writer
suffix. Basic, extended, critter, lock, container, map, script, and combat flags
have separate types and preserve uninterpreted bits.

`MapFile::parse` takes a `PrototypeLookup` provider. `LocalPrototypeCatalog` resolves
one-based prototype-list entries, caches subtype/default information, and uses
the selected file's PID to decode it. Maps retain embedded MVAR/LVAR pools for
later engine initialization; no campaign GVAR array or invented GAM parser is
involved. Negative header pool counts consume zero words. Tile omission flags
omit only the corresponding tile block, and roof-hide bits clear on loaded tiles.

All 16 script slots in each block are decoded using their individual category,
including padding slots. Active occupancy, local slices, and unique active script
IDs are checked. Object trees are consumed depth-first, quantities represent
one child tree, and enclosing elevations/owners are restored. Script bindings,
outlines, critter flags, stock ammunition, and exit-grid art receive the specified
loading adjustments. Obsolete pointer words never become offsets or live pointers.
Limits are 100,000 scripts, 100,000 total objects including children, and inventory
depth 64. Counts and actual read extents are checked before allocation.

`HexGrid` provides bounded neighbors, border-stop stepping, both distance
definitions, camera centering, and integer hex/floor/roof projection/picking.
Negative inverse coordinates use floor division. Under the SPEC-003 §3.5 safe
rule `angular_distance` returns the axial lattice distance for every valid pair;
the reference's walk (`angular_walk`) agrees wherever it stays on the grid and
leaves it for pairs such as `(0,199)`. Only the explicit `-1` sentinel gives 9999.
Multi-step movement with an orientation outside 0-5 returns the starting hex.

## Core VM

`IntProgram` validates the 42-byte executable bootstrap region, procedure records,
identifier/literal records and terminators, and complete instruction boundaries.
Procedure addresses are absolute image offsets and must select executable
instruction boundaries, with one loading exception: a procedure body/condition
may equal the exact image length to represent an unused trailing stub. Jumping,
calling, or evaluating a condition at EOF still raises a runtime address error;
addresses beyond EOF remain parse errors. Identifier-consuming operations resolve
their raw operand against the identifier table even if pushed with a string tag;
literal bounds are enforced when a value is consumed as a string. The image is
bounded to 64 MiB, procedure count to 65,536, and declared arguments to stack capacity.

`Vm` implements all 76 core dispatch slots, including distinct raw/type-dependent
truth rules, asymmetric integer-addition promotion, float-bitwise conversion,
context returns, and mutable scheduling metadata. Under the SPEC-004 safe rules
`0x802E` discards exactly its counted values (the fixed-count rule; the reference
reuses each popped payload as the next bound) and a timed wait releases on its
first readiness check. Both stacks have 2048 bytes and fit 341 tagged values.
Frames and globals use byte bases, with checked live-slot access. Strings are
interned per program with an 8 MiB content budget; cross-program transfers copy text.

`update` uses ten instructions by default. Critical state can continue beyond the
ordinary budget; a separate 100,000-instruction hard limit prevents an unbounded
critical invocation. The SPEC-004 §3.5 safe rules define the remaining cases:
integer subtraction, multiplication, negation, and division wrap modulo 2^32
(`MIN / -1` is `MIN`), out-of-range or NaN float-to-integer conversions give
`80 00 00 00`, nonfinite floats format as `nan`, `inf`, and `-inf`, and every
operation that would consume its operands and push nothing (a string on the left
of arithmetic or bitwise operators, a string in the float-to-integer conversion)
raises a VM type error, as does a non-integer slot index or frame count.

`VmHost` supplies time, shared exports, program ownership/launching, wait predicates,
and extension opcodes `0x804C..=0x8155`. `BasicHost` supplies a deterministic clock
value and case-insensitive export registries. No process-global state or filesystem
access is implicit in execution. Host calls and external return contexts are
available explicitly; imported calls are rejected with `ImportedAbi`, the reject
choice the SPEC-004 §3.6 safe rule allows.

Lifecycle resolution preserves the generic index-1 fallback and the separate
map-enter/map-update presence checks. Scheduled conditions execute against the
real VM/host and can have side effects; no pure-procedure flag is invented.
Timed deadlines use strict unsigned comparison, condition flags take precedence,
and firing/canceling clears the specified metadata words.

## World state and host extensions

`GamFile` applies the exact 16-byte section comparisons and 257-byte translated
chunks. Names do not affect indices, only column-zero `//` is a comment, and later
section headings remain data. Missing markers yield an empty initializer. Under
the SPEC-006 §2.1 safe rule an assignment with no convertible number stores zero
and an overflowing decimal is clamped to the signed 32-bit range.

`GcdFile` decodes all 428 bytes with checked tags/traits, name termination, and
raw SPECIAL bounds. `player_normalized()` applies player-only EMP/body/reward/
kill-category changes; `CharacterSheet::from_gcd` then recomputes derived bases
and performs the bounded initial healing request.

`CharacterSheet` separates trait-adjusted raw attributes R from effective,
bonus-adjusted E. It implements all 16 trait selections, the SPEC-011 perk-effect
mechanism (ranks, armor perks, and withdrawal perks), tag investment,
difficulty, and accumulated level-up HP history. Perk effects are applied on
rank changes, not on each query. `ParkMiller` exposes a complete shuffle-state snapshot, preserves
entry zero on reseeding, distinguishes seed/draw sign correction, and supports
explicit startup diagnostic draws. Checks preserve the tick-864000 boundary and
their exact number of random draws.

`WorldState` owns generational entities and separate opaque VM references,
GVAR/MVAR/LVAR storage, the ten-tick clock, ordered timed events, lighting,
script contexts, and dialogue. Script requests distinguish their first bootstrap
execution from later named calls. Map loading records and executes the ten
specified stages, including full ambient before entry and player light after
the initial map-script request. Timers use stable insertion order at equal due
times; spatial callbacks are checked on each animated movement step with reentry
suppressed. Inventory stack splits preserve a distinct removed representative
(SPEC-011 §3.13).

The extension ABI is compiled from the approved SPEC-005 table at build time.
All 266 slots are present: 264 registered words and holes `0x807D`/`0x807E`.
`vault-vm::extensions` preserves raw tags, uses mask `0xF7FF`, implements the
I/S/F/U/O/N/C/T checks and variable region arity, and does not synthesize statement
results. `WorldHost` connects the catalog to world services and managed-library
state. Documented stubs, defective tokenization, and wrong-string-tag paths are
explicit; the SPEC-005 `[May-improve]` rows apply their safe rules (listed
below under "SPEC-001 to SPEC-008 safe rules"), so null operands push the row's
null result, the host-busy bit is cleared on every return path, area visibility
terminates, and time arithmetic wraps at 32 bits.

Exports use separate 1013-entry registries. Variable ownership is by registered
program filename; same-owner registration resets to integer zero. Procedure
exports are removed on program purge, while variables survive until registry
cleanup. Named callbacks, keyboard bindings, and script timers retain their
separate lifetimes.

`GDialog` tracks session, node processing, review, paging, callbacks, and head
animation separately. Replies clear old options, IQ filtering happens at append
time, options are limited to 30 (a thirty-first is a diagnostic), review replies
to 80, and literal replies and options are truncated to 899 and 897 bytes; every
reply page is remembered. Head/background IDs, filename suffixes, own-column
fidget counts (kept only when every selectable fidget file exists),
reaction transitions, and the 42 phoneme mappings follow SPEC-006. The platform
renders loaded head/background/interface art and font glyphs; no fabricated
conversation is substituted for a missing NPC script.

Host boundaries remain explicit where the approved specs do not define the
underlying tables or media formats. `WorldRules` supplies the medical-kit PID
roles, campaign reputation indices, the calendar base year, and numeric action
codes not enumerated by the specs; it can override the SPEC-011 money PID (41)
and weapon-perk numbers (58-61). `WorldAssets` supplies sound-name codecs and
localized continuation labels. Absent mappings are diagnosed or rejected, rather
than guessed from reverse-engineering sources. Unobstructed light arithmetic is
exact; occlusion/shot lines use the engine's bounded integer hex-line policy.
Audio, movies, general managed-window drawing, combat requests, and unspecified
item/medical defaults are typed host presentation/action requests. The map preview
implements world interaction, GDialog rendering, sound, and movies, not a full
combat/medical subsystem.

## Art identifiers, routing, and combat (SPEC-007)

`Fid` packs base/weapon/animation/category/selector fields; `ArtCatalog` parses the
category lists and implements the exact critter suffix table (weapon codes 0-10
checked before every branch, single alias substitution for animations
27/29/30/33/55/57/58/64, weapon actions requiring a weapon), head suffixes, and the
constructor's directional fallback (only critter animations 20-35 except 33 probe
the requested selector, then `.fr0` once; the original base is retained).

`routing` implements the heuristic best-first route search exactly as recovered:
seen-on-insertion, first-vacant frontier slots, 50 per hex plus 10 per direction
change outside combat (initial direction 0), 1999-entry frontier/expanded caps,
and an 800-byte backtrack that, under the SPEC-007 §3.1 safe rule, keeps the
first 800 steps from the source. The movement blocker covers
critter/scenery/wall art on the hex plus multihex neighbors; non-player
biped/robotic critters auto-open unlocked doors. Rays use pixel Bresenham between
hex centers with stride sampling, the 200-sample cap, and projectile-through
filtering at stride 32; the first eligible object on a hex masks the others.
Reaching the sample cap clears the obstacle output (safe rule).
`obj_can_see_obj` combines the sector perception rule with a stride-16 ray whose
first blocker must be the target (the ignored reference is explicitly null).
Move-to-object/tile planning shortens routes and derives the recovered replan
destination; execution opens usable doors, rebuilds blocked routes with a strict
goal, and charges 1/4/8 AP per hex (free movement first) in combat.

`combat` holds the explicit attack arithmetic: AP costs and ranges, the exact hit
chance (no lower cap; ammunition AC/DR/multiplier/divisor are not consumed), the
first-day-gated critical upgrade and forced upgrades, the 864 critical-success
entries and failure masks (compiled from the approved specification at build
time), per-round DT/DR damage, knockback that stops one hex short of the first
blocker (safe rule; the reference's double decrement stops two short),
burst/continuous sprays (budgets, direct trials, side rays, extra targets at
normal scale), death-animation selection (a rear hit falls back to 21 when
fall-front art exists, else 20, under the safe rule), and corpse rules.
`combat_state` implements initialization, the first unsorted pass, the round
sweep, turns, explicit end requests, automatic ending, and cleanup. Critical-
failure self damage and its flags are committed to a living attacker (safe rule).
AI behavior is unspecified, so NPC turns and danger targets come from a
host-supplied `CombatAi` (default: none). The weapon-splash helper adds no victims
(its setup is absent in the reference), and the miss-impact fallback beyond
grenade scatter is not specified enough to simulate. A single-shot ranged miss
can hit an accidental target under the safe rule (see the next section).

Registered animation sequences now execute: commands run in order, one frame step
at a time (the art's frame interval, 10 FPS without art); nonnegative delays wait
that many frame steps; moves follow SPEC-007 routes with unlimited AP; forward
death animations end on their static art; completed sequences retire (returning
critters to standing unless flag 4), so `anim_busy` reports `-1` only while a
nontrivial registration is active.

## Archives, saves, world travel, and Pip-Boy (SPEC-008)

`DatArchive` reads the byte-zero root and per-directory tables (one-byte name
lengths, big-endian words, `data_size` of at least 16) and extracts members by
attribute word: exactly zero or `0x10` is the legacy single LZSS stream whose
output must equal the logical length, `0x20` is raw, and `0x40` is the blocked
form (two-byte headers; every block must produce exactly
`min(16384, remaining)` bytes). The LZSS decoder uses the 4096-byte ring with
4078 leading spaces, cursor `0xFEE`, LSB-first control bits, absolute 12-bit
starts, and sequential overlapping copies. Truncated tokens, over-expansion,
short or empty blocks, and offsets past EOF are errors rather than unbounded
reads or loops. The reference leaves the ring tail uninitialized; under the
SPEC-008 §3.1 safe rule it is zeroed at the start of every compressed block,
which only matters for streams that read the tail before writing it.
`Database` and `DatProvider` implement selected-database lookup: the loose patch
directory first, then that database's archive, with `art/critters/` selecting
the critter database and no fallback between databases. `VfsWorldAssets` and
`LocalPrototypeCatalog` accept any `Vfs`, so the world host runs from a loose
directory or from `DatProvider` archives. The desktop shell's `--corpus-dir`
still mounts a loose directory.
`DatArchive::open` (a file) and `DatArchive::from_reader` (any seekable
stream) read only the directory tables when the archive is mounted. Each
`extract` then seeks to one member, reads only its stored bytes, and decodes
them, so a mounted archive holds just its tables in memory. Clones share the
stream, and reads are serialized. `DatArchive::parse` still takes the whole
archive in memory, and both forms extract identical bytes.

`savegame` reads and writes the strict 30051-byte header (only the first 18
signature bytes are compared; a version other than 1.1 is rejected under the
SPEC-008 §2.4 safe rule) and all 27 ordered
payloads with no length prefixes. The campaign count G comes from the installed
initializer; the queue grammar uses type-dependent payloads; manifest names are
bounded to 12 bytes before the NUL; trailing bytes are rejected. `AutomapDb`
holds `MAPS/AUTOMAP.DB` (version 1, the 797-byte empty table with the excluded
map set, LZSS rasters that stop at 10000 output bytes, and the `X = 200 - column`
raster writer that spills column zero into the next row). Captures are stored as
raw entries, since the specification defines no encoder: rewriting an entry
shifts later offsets and the recorded length, and appending requires EOF to equal
the recorded length.

Leaving a map writes its `.SAV` image (saved bit set, live globals/locals,
scripts, and placed objects with inventories) into an in-memory working
saved-map set and captures its automap rasters. Entering a map prefers that image
over the asset. The player is saved centrally. Party members travel with the
player between maps but are written to the current map in a save slot. The
per-load map-script instance is recreated on each entry, as in SPEC-006 step 6,
so its previous local block is not reattached. `WorldHost::save_game` writes
`SAVEGAME/SLOTnn/SAVE.DAT`, the manifest's `.SAV` companions, and `AUTOMAP.SAV`
under the SPEC-008 §3.3 safe rule: every file is staged as `<name>.TMP`, and only
after all succeed are the old files renamed to `.BAK`, the staged files renamed
into place (restoring the backups if that fails), and the backups removed.
`load_game` reads and validates the header, every payload, every companion map
(parsed in full), and the automap before it changes any state. A timed event
whose owner identifier matches no restored object is restored without an owner. It then restores in the reference order: reset, game
tick, campaign snapshot A, working set, current map, then the player and later
components (snapshot B is ignored). An active saved combat resumes the player's
turn without an AP refill. The saved next-identity counter is installed before
the map load, so temporary objects cannot take saved inventory identities.
Payloads the engine does not model are carried verbatim: preferences outside
difficulty, combat difficulty, violence, and burning-run; movies; skill
timestamps; and interface words. Component 10 holds the selected rank of each
SPEC-011 perk index 0-63, and component 19 the reconciled level and the sheet's
pending-perk flag. The platform supplies
the thumbnail and host date/time (`SaveInfo`). The base calendar year is
`WorldRules::calendar_year` (default 0), because the specs define only the
non-leap month/day calendar.

`worldmap` compiles the terrain, frequency, and region grids, the town table,
and the hotspot catalog from the approved specification at build time. It
implements the asymmetric pixel stepper (the X-major branch can finish one Y
pixel short), W/Q travel speed (binary32, with the Pathfinder factor in binary64;
Outdoorsman is clamped below at 0 and Pathfinder ranks to 0-4 under the safe
rule), the terrain schedules on one shared counter, the
29-stride discovery reveal (3 x 3, or 5 x 5 with Scout, plus the west/ocean
thresholds), 3d6 encounter classes with town suppression, special encounters
(restarting the whole test when the drawn special is consumed), terrain-map
redraws, globals 65/32, the town-knowledge refresh (known value exactly 1,
full 32-bit entrance tests under the safe rule, Vault 13's tick rule, alternate
rows 12-14), buttons,
labels, and first entry. `WorldHost::travel_iteration` runs one loop iteration:
schedule steps and walk-mask stops, then the Q charge with queue interruption,
discovery every third iteration, then 24-hour party healing and the encounter
check every W iterations. Script-directed town travel skips encounters and the
mask and charges no city time. `vault-guidance` derives holodisk lists, pages,
and quest status from the fixed tables. Pip-Boy rest (`WorldHost::rest`)
implements eligibility, the A/N/K step allocation, queue-interrupted steps, the
strict final catch-up, credited minutes with three-hour healing at 180,
until-healed (including the extra chunk after full HP), and wake-up presets.

Where SPEC-008 leaves details open, this engine makes these choices:

- A desert/coast iteration resets the terrain counter to zero, and the mountain
  and city schedules treat a nonpositive counter as expired.
- Within an iteration, steps come before the time charge, discovery, and the
  interval check.
- Town and destination jitter uses one signed `-16..=16` draw per axis.
- Travel time charges never move the clock backwards.
- The walking-mask contents are host-supplied; without a mask nothing blocks.
- Automap capture uses objects' existing seen flags and runs at every map save.
- Town knowledge refreshes on town entry.
- Rest arithmetic uses binary64.
- Map-exit script request failures are diagnostics, like map-entry failures,
  so a missing compiled map script cannot trap transitions.

## Combat AI (SPEC-009)

`vault_formats::ai` reads `data/ai.txt` with the configuration grammar: 255-byte
line chunks, `;` truncation anywhere, `[...]` headers that end the line, the
first `=` split, 0x20/0x09-0x0D trimming, case-folded sorted sections that exist
only once they hold a key, merged repeats with the last value winning, and the
first spelling kept. Every packet needs all 25 keys. Integers use decimal-prefix
parsing; `hurt_too_much` uses the space-skipping, trailing-space-sensitive token
grammar. Keys before any header form section `unknown` (reset per file), an
embedded NUL is rejected, out-of-range integers saturate with a diagnostic, and a
missing file or empty table fails initialization. Resolution scans the sorted
table at every query and falls back to entry 0 with a diagnostic.

`WorldHost::load_combat_ai` loads `combatai.msg`, then the packet table. The
built-in `PacketAi` is the default combat policy: it runs `WorldHost::ai_turn`
and uses the live `danger()` for the join and stop predicates. Combat start
collects critters in ascending hex order, copies them into the snapshot, and has
the initiating pair reference each other through the setter. Distance sorts are
stable and persist in the snapshot; destroyed critters are swap-removed. The
setter refuses non-critter art and writes reaction 1 into script local 0 for the
player. Damage application alerts every perceiving snapshot entry and applies the
setter/retaliation rules (the player's attack debit references the defender).
The NPC turn implements forced flee, healing, the requested target with its
accuracy bonus during the first turn, the max-distance stop, the hurt-and-
unopposed flee, and party following; flee runs relative to the player through
strict-goal candidates; weapon usability, preference, inventory and environment
search, retrieval, switching, hit-mode and called-shot selection, the ten-attempt
loop, and reloading follow the specified draw order. Random-hit critical failures
retarget through the snapshot (torso, critical cleared, hit marked, one normal
round). Taunts draw at the request sites, fill the two 79-byte buffers, and
display at callback time unless a floating text exists.

Where SPEC-009 leaves details open, this engine makes these choices:

- NPC actions run synchronously within the turn: movement, attacks, and an
  action's taunt callbacks complete before the next decision. Delay-0 and delay
  -1 callbacks run in registration order when the action finishes.
- Movement "limited to the current AP" truncates the route to that many hexes;
  each committed hex still charges its leg-dependent cost, saturating at zero.
- NPC healing consumes each unit through the SPEC-011 §3.7 contract
  (`WorldHost::consume_drug`). No approved specification defines the SPEC-002
  language filter: its preference state machine is tracked
  (`AiState::filtered`), but catalog text is unchanged.
- Party membership applies the 18000 object-identifier floor literally. Object
  identifiers come from map and save data and the insertion counter, so members
  with lower identifiers never follow.
- An unset combat-difficulty preference reads as 0 for SPEC-005's opcode and
  SPEC-007 hit chances (as before) and as 1 for SPEC-009 called shots.
- Wielding sets the critter's art weapon code to the weapon's animation code (the
  "visibly holds a weapon" test reads it), and unwielding clears it; take-out
  (38) and put-away (39) are presentation requests. AI pick-ups run the item's
  pick-up script first and present `DefaultAction::Pickup`.
- Team switching does not rebuild combat outlines, which have no approved
  encoding.
- Restoring an in-combat save rebuilds the snapshot from the restored combat
  list, but last-attacker references are not rebuilt from saved combat
  identifiers.
- Explosion attribution and explosive-drop noticing are host entry points
  (`attribute_explosion`, `notice_explosive_drop`). Nothing calls them yet:
  SPEC-007's weapon splash adds no victims, and SPEC-013 §3.10 explosions
  (timed and scripted) damage only the center critter and credit their source
  themselves. No approved spec names the explosion art, so an animated
  explosion stays in progress until the next world update.
- `float_msg` texts count as floating text for taunt suppression, with the same
  default lifetime and 200-pixel wrap.
- Fixture H lists R2's rating as 22. By the §3.4 rule it is `max(2, 20) + 0 = 20`.
  Either value keeps Q.

## Sound effects and ACM audio (SPEC-010)

`vault_formats::acm` reads the 14-byte little-endian header, then decodes the
LSB-first bitstream with zero bytes supplied past EOF (look-ahead included).
Each block rebuilds the asymmetric 16-bit step table and decodes every column
code (0, 3-24, 26, 27, 29). The lifting carries persist across blocks, and the
level-1 carries wrap to 16 bits. The +1 bias and the row chunks of
`max(1, floor(2048 / C) - 2)` follow the spec. Output is `wrap16(e >> L)` (wrapped, not
saturated), and decoding stops at N samples. Reserved formats, out-of-range table
indexes, `R = 0` with samples to emit, and a block header at or past EOF are
errors. Decoded samples are bounded by the 64 MiB asset limit, and blocks by 4 Mi
elements. Effects and speech play mono at `acm::MIXER_RATE` (22050 Hz) whatever
the header declares.

`vault_formats::sfx` builds character, weapon, open/close, ambient, interface,
and scenery names. It implements the lookup chain (H/N gender aliases with the
male fallback, the case-sensitive `MALIEU`/`MAMTN2` alias), speech and narration
paths, and the relative-volume formula. In the world host, the open/close
builder reads the item sound byte at prototype offset 0x38 and the scenery byte
at 0x28. Weapon names read the weapon byte at 0x79 with the target's material.
Character names use the unaliased art-list token (`WorldAssets::art_token`).
Opcodes `0x80A5`, `0x813D`, and `0x8141` call these builders, and the SPEC-009
AI's ready and out-of-ammunition requests carry their names.
`WorldHost::effect_file` resolves a name with an optional owner. `speech_file`
uses the active talking head, and `WorldState::effect_volume` gives the relative
volume.

Where SPEC-010 leaves details open, this engine makes these choices:

- "No name" results (an invalid action or effect type, a missing art-list row, a
  failed suffix rule) push D offset zero with a diagnostic, where the reference
  copies unchecked text.
- A non-scenery, non-item object uses item sound byte 0 for open/close names,
  since only item prototypes carry that byte.
- Narration names use `/` separators in the VFS path.
- The core resolves names and relative volumes only; mixing is the platform's.
  The desktop shell's four-voice mixer (`vault-platform`'s `audio` module)
  models the four-effect limit, refusing a fifth request rather than evicting
  one, and plays each effect at its owner's `WorldState::effect_volume` (see
  "Desktop game shell").

## Items, drugs, barter, and progression (SPEC-011)

`vault_core::drugs` holds the pure rules: the fixed addiction table, stage slots
(the -2 range form draws once at the first processed slot, and a skipped slot
moves the draw on), delayed-stage scheduling, and the player's Chem Reliant, Chem
Resistant, and Flower Child scaling. `WorldHost::consume_drug` runs §3.1:
dead or robotic consumers give -1 with no draw; otherwise the re-dose scan, the
immediate stage, both delayed stages (queue type 0), and the addiction roll,
which is drawn even at 0%. NPC bonus changes apply to the critter prototype:
`WorldState::adjust_bonus` updates every instance of the PID and records the
arrays in `prototype_stats` for instances created later. Current HP clears the
fleeing bit, caps at maximum HP, and kills through the kill path at or below 0.
Poison and radiation act only on the player. The player sees `item.msg`
gained/lost lines, or "nothing happens". Withdrawal onset and end events apply
and remove the perk effect, print the perk description and `item.msg` 3, and stop
the queue pass for the player (interrupting travel and rest). Map exit applies
non-party stages and end events (§3.9). SPEC-009 healing consumes through the same
contract. The inventory use (`use_drug`) and use-on paths follow §3.6.

`vault_core::items` implements §3.13-3.16 on `WorldState`. Stacking compares PID,
script, equipment and used flags, and instance state, ignoring ammunition rounds
and the money amount. An added item becomes the entry's representative (ammo
packs merge with the documented front/quantity rule). Removal clones the
representative, resetting ammunition to full packs. Item weight and value cover
containers, loaded weapons (whole packs; `L × cost / K`), and ammunition entries.
`add_refusal` applies the biped carry (`>`), body-type, container (`≥`, plus the
outermost carrier), and power-cell limits; forced transfers skip them. Reload,
search reload, drag reload, and unload follow §3.15. Armor contributes AC, DR, DT,
and its perk to the player only (`WorldHost::wield_object` also sets the armor
art); an NPC's worn armor sets the flag only. A player attack below a weapon's
minimum strength prints `combat.msg` 107, and opening the inventory in combat
costs `4 - Quick Pockets` AP (`open_inventory`).

`vault_core::progression` holds the experience table (maximum level 21), the
Swift Learner award, level-up HP, the 64-row perk table (ranks, levels,
requirements, effects, and the effect-only perks 53-63), eligibility, the
character-screen reconciliation with its single pending-perk flag, and the
chooser extras (Tag!, Mutate!, Lifegiver, Educated). `Perk` is numbered by perk
index, so saves, `has_trait`, and `critter_add_trait` use the table directly.
`WorldHost::give_experience` prints `stat.msg` 600 and plays `levelup` per level.
`vault_core::barter` evaluates the binary64 price formula, the reaction modifier,
and Master Trader. `BarterSession` (`begin_barter`, `barter_move`,
`barter_price`, `attempt_trade`, `end_barter`) hides the merchant's armor and
right-hand item or first weapon, applies the acceptance rules, and returns
everything on exit.

Where SPEC-011 leaves details open, or offers a safe rule, this engine makes
these choices:

- The `[May-improve]` safe rules apply. The addiction test reads the mapped
  variable 189-194, and so does the end event's "any addiction" test. Inventory
  use destroys a unit only on result 1. Barter uses `M_s + M_r` from the first
  frame. The player-stat setter rejects selectors outside 0-4.
- Stat names in the gained/lost lines are `stat.msg` 100 + selector, the
  catalog's 38 name entries.
- An overdose uses the kill path's fall-back death (animation 20).
- The use-on messages 582 and 583-586 show only when the player is the actor.
  The dead-target `roll(0, 3)` is drawn for every actor.
- A cancelled Tag! or Mutate! sub-selection keeps the pending pick, because the
  choice did not complete.
- Prototype stat changes last for the session. Loading a save rebuilds the world
  from the save and the prototype files, because SPEC-008 saves no prototypes.
- A loaded weapon whose ammunition prototype was never seen (by object, import,
  or reload) weighs and costs as unloaded. Unloading it creates nothing.
- Fixture A3 says its stage delays are unchanged. A3 includes Chem Resistant,
  and §3.3 halves delays for it, so they are halved as in A2.
- The inventory screen's held-outside-the-list item is not modeled. The
  desktop's inventory, character, and barter screens (see "Desktop game
  shell") present these rules. Their skill and trait labels are built-in
  English names, since no approved specification names their catalogs.

## Movies and subtitles (SPEC-012)

`vault_formats::mve` validates the 26-byte little-endian header (all twenty
signature bytes, header size 26, version 256, `version + check = 0x1233`). It
then reads chunk records of `L + 4` bytes (the body plus the next chunk's
header, so a file ends with a trailing header) and processes opcodes in
stream order. `Movie::next_event` reports timers, audio formats, sync points,
track-0 audio (opcode 8 data, opcode 9 silence; mask bit `0x0001`), shown
frames, and the end; video configuration, palette loads, the suppress marker,
the decoding map, and video data take effect internally. The block decoder
implements all sixteen types: the low-nibble-first map, one parameter stream,
the near/far motion tables with linear source addressing (an x position past
a row edge wraps into the neighboring row), copies four bytes at a time from
the top, and every two- and four-color subcase. Audio restarts its predictors
per opcode and adds the §2.8 deltas with wrapping 16-bit arithmetic.

`vault_core::movie::MoviePlayback` runs a movie against host microseconds:
`T = rate × subdivision + (subdivision >> 1)`, the same-period no-op, sync
waits until `deadline + T/4`, late frames (more than `T/2` late, never right
after a dropped frame), dropping only frames without a palette push, and the
final deadline wait. Palette pushes reach the device directly or through the
`vault_formats::moviefx` fade hook. That hook reads the fade file with the
SPEC-009 configuration grammar, searches effects newest-loaded first, and
writes truncating `in`/`out` levels modulo 256 over the whole palette.
`vault_formats::sve` keeps subtitle entries in file order and draws every
leading entry whose number is at most the frame counter after each step.
`vault_render::movie::MovieWindow` copies displayed frames centered (432 x 320
at 104, 80) and draws the subtitle band.

`WorldHost::play_game_movie` looks the movie up (`art\cuts\<file>`), picks the
subtitle file (`text\<language>\cuts\<name>.SVE`) and fade file, marks the
SAVE.DAT component 22 byte, and emits `WorldEffect::Movie`. A missing movie
(always `raekills.mve`) fails and leaves its byte unchanged. `play_gmovie`
uses the §2.11 flags plus `0x02` for indexes 3 and 4 without a map; the
endgame plays `walkm` or `walkw` by gender; `play_archive_movie` implements the
Pip-Boy scan. The subtitles preference is SPEC-008 preference word 9.

Where SPEC-012 leaves details open, or offers a safe rule, this engine makes
these choices:

- The `[May-improve]` safe rules apply. Timer rates use all 32 bits. Any of
  these is a stream error that ends the movie (and any library error ends
  it): an opcode or payload past its chunk body, video data without a map in
  its chunk, block type 6, parameter over-reads, motion sources outside the
  buffer, odd region widths, palette ranges past 256 entries, high-color
  movies, height multipliers other than 1, and audio lengths shorter than
  their predictors. New buffers are zeroed. Opcode 11 is ignored and
  uncompressed audio copies its payload. Field mode displays normally,
  subtitle lines are read whole, and a `play_gmovie` index outside 0-13 or an
  empty Pip-Boy archive plays nothing.
- §2.8's prose places -29973 at code 121; the table (and Fixture E, code 137,
  and the oracle) put it at code 120, so codes 120-127 are negative.
- The device audio scheduler is replaced by one stream, fed in decode order,
  that starts at the first sync point after the audio configuration. The
  movie volume is `trunc(0.94 x music)`. The attenuation
  `trunc(-1000 x log2(32767 / v))`, in hundredths of a decibel, becomes gain
  `10^(A/2000)`. The desktop has no background music, so it uses the full
  music volume, and the music flags (`0x04`, `0x08`) have no effect.
- A partial decode region is copied from its offset in the frame to the
  centered frame origin. Subtitle text wraps at the window width, with each
  line centered and the first line 2 pixels below the band top.
- The desktop does not present script palette fades, so after a movie
  without flag `0x02` the game palette returns at once instead of staying
  black until the caller fades in.
- No approved specification names the remaining-water global or the
  world-map event triggers. The midnight countdown runs only with
  `WorldRules::water_global`. While the water-chip quest (global 101) is at
  most 1, it decrements the water. Then, outside dialogue, the most severe
  threshold reached plays its movie if not yet played: boil3 at 0 or less
  (`0x05`, then `WorldEffect::EndGame` as the quit request), boil2 at 50 or
  less, and boil1 at 100 or less (`0x07`). The world-map events can call
  `play_game_movie` directly. There is no main menu or new-game flow, so the
  main-menu intro and `ovrintro` callers are not wired.
- Movies play modally, so `play_gmovie`'s suspension of dialogue background
  processing is the modal wait itself.

## SPEC-001 to SPEC-008 safe rules

The audited SPEC-001 to SPEC-008 revisions tag every quirk `[Must-match]` or
`[May-improve]`. The engine applies the stated `[May-improve]` safe rules,
including:

- SPEC-001: FRM payload/dimension/offset rejections (above); LST entry counts
  come from successful bounded reads; head lists read the good, neutral, and bad
  fidget counts from their own columns, keeping a column only when every fidget
  file it can select exists and otherwise the first-column count.
- SPEC-002: selecting an unloaded font is a no-op; the AAF maximum-cell metric
  is `H + S` whatever Q; MSG and font validation as described above.
- SPEC-003: PIDs with nonzero bits 28-31, stored PIDs that differ from the
  requested PID, unknown subtypes, and malformed script/object counts are
  rejected; attack-mode nibbles 9-15 read as mode 0; the PRO writer keeps reader
  order (minimum then maximum damage, no scenery suffix); a map entry
  orientation outside 0-5 reads as 0 with a diagnostic; LVAR access is bounded
  to the script's own slice and system/map scripts cannot write locals;
  temporary inventory children are skipped when writing.
- SPEC-004: the arithmetic, conversion, slot-index, discard, and wait rules
  above.
- SPEC-005: `saystart` indexes after incrementing; `sayreplyflags` sets reply
  flags; `printrect` truncates to 79 bytes and prints nothing for an unsupported
  tag; `playmovie` waits until playback ends (the desktop now plays library
  movies and reports their end); non-S region names and button images are
  rejected or treated as no image; `addbuttonrightproc` targets the button;
  `tokenize` returns I zero for an absent match and never passes the terminator;
  `obj_name` of null is `<Critter>`; `create_object_sid` pushes 0 when nothing is
  created and treats script number 0 like -1; `tile_num_in_direction` gives -1
  for a negative orientation; `pickup_obj` without a script is a diagnostic;
  `turn_off/on_objs_in_area` swap reversed hex bounds and advance one elevation
  per pass; `obj_can_hear_obj` tests both objects and perception;
  `critter_attempt_placement`, `critter_inven_obj`, `rm_mult_objs_from_inven`,
  `destroy_mult_objs`, `critter_heal`, and `critter_rm_trait` push their null
  results; `rotation_to_tile` gives 0 for an invalid hex; `start_gdialog`
  derives the mood from self without a prior target; `attack`, `attack_setup`
  (a null defender is a no-op), and `critter_damage` clear the busy bit on
  every path; `game_ticks` and the time opcodes wrap at 32 bits; debug text is
  literal; `sfx_build_char_name` pushes empty text when unavailable and
  `sfx_build_weapon_name` rejects out-of-range effects and hit modes; negative
  perk/trait indexes give 0; nonfinite color channels read 0 and channels clamp
  to 0-31; named-event names truncate to 31; stray sound handles are no-ops; the
  any-key binding is cleared with its program.
- SPEC-006: GAM values (above); literal reply/option truncation, the
  thirty-first option as a diagnostic, and unbounded reply paging; a head
  outside the list, a fidget count of zero or less, or missing head art selects
  no fidget, keeps the current frame, and is diagnosed (the fidget draw is still
  taken); a range whose width does not fit 1 through 2147483647 consumes one
  draw and returns its first bound.
- SPEC-007: failure tables outside 0-6 use table 0; routes keep their first 800
  steps; the ray sample cap clears the obstacle; combat begins only when every
  supplied participant is on the current elevation; the inventory entry cost is
  clamped at 0; plasma throw weapons cannot aim; the request override words go
  to the attacker and defender flags; knockback, the rear death fallback, and
  attacker self-damage as above. On a single-shot ranged miss, the first
  non-through critter other than the defender on the stride-32 shot ray (the
  `obj_can_see_obj`/cover ray) is accepted as the accidental target and takes a
  torso hit at normal damage, like a random-target critical failure.
- SPEC-008: the ring tail, version check, staged saves, companion validation,
  travel clamps, entrance test, and live loose reads above; discovery resets
  clear all 31 x 29 bytes; holodisk last pages use `floor((n - 1) / 35)`; only
  positive acquisition values list a disk; every quest pass stops at the zero
  sentinel; an until-healed rest with a nonpositive healing rate does nothing.

Deliberate exceptions and gaps:

- The SPEC-001 §3.2 rule rejecting a `color.pal` shorter than `0x8304` bytes
  is not applied: every corpus palette (19 files, `color.pal` included) is the
  33,536-byte base layout without a discriminator, and `[May-improve]` rules are
  optional.
- MSG loading keeps the tolerant recovery described above (`parse_strict`
  applies the SPEC-002 rules), and `TextMode::WindowsLoose` remains the
  reference-behavior switch for loose-file Ctrl-Z.
- SPEC-005's optional rule rejecting invalid nonzero object references with the
  row's null result is not applied; such references remain VM errors.
- SPEC-007's selection and damage of the accidental target are not specified;
  the rule above is this engine's choice. The `[Must-match]` one-in-four
  punch-to-kick cosmetic draw is not modeled, because the spec gives neither its
  bounds nor its place in the draw sequence.
- The SPEC-003 mapper inventory-repair mode is omitted, as the safe rule allows.

## Spec completion notes

These close behaviors that the earlier phases left open:

- SPEC-001: the gamma display formula (`display_channel`), an opaque display
  palette (index 0 is transparent only in blits), cycler fade suspension, and
  `FrameCursor` forward/reverse displacement accumulation.
- SPEC-002: `FontSet` startup search (FON 0-9, AAF 0-15), per-subsystem defaults,
  numbered selection (FON 0-9, AAF 100-110) that never replaces the current font
  with a missing slot, `setfont`, and the numeric text-flag word.
- SPEC-003: `Prototype::write` and `MapFile::write` (darkness 1, recomputed
  omission bits, grid-order top-level saving without temporary or unplaced
  objects, `NO_SAVE` scripts omitted, temporary inventory children skipped),
  extended-flag interaction permissions, and out-of-range MVAR/LVAR diagnostics.
  The roof-hide flood fill is named but not specified; unknown item/scenery
  subtypes are rejected, as the safe rule now states.
- SPEC-004: scheduled procedures fire from the host's interpreter pass, and
  `launch` starts attached, replacing, spawned, and independent programs.
- SPEC-005: executing animation sequences, armor-aware `critter_damage`, the exact
  `obj_can_see_obj`, fleeing-target rejection, `anim` 1000 controls, loaded
  palette tables, retained button graphics, named-event busy state, and
  consistent 31-byte export keys. Sound-name builders follow SPEC-010.
- SPEC-006: elevation-change map updates, speech completion, the queue
  stop-on-nonzero rule, reputation inputs (campaign indices supplied by
  `WorldRules`), talk reachability/12-tile checks, campaign-open failure, and
  map-load and map-exit script failures as diagnostics. Map loads now consult
  the SPEC-008 working saved-map set before the asset lookup.

- SPEC-007 R1-rev2: a punch request first draws D(1, 4) and, on 1 with kick
  art, becomes a kick; a ranged or throw miss resolves bystanders through
  Stage A (the line-of-fire D100s; under the safe rule a selected critter is
  committed) and Stage B (the impact hex, with grenade scatter drawing the
  distance before the direction), one round at numerator 2, the "instead" and
  "missed" messages, and bystander retaliation only across teams. An unset
  combat difficulty now counts as normal (SPEC-014 §2.3).
- SPEC-001 R1-rev2: a 33536-byte palette is the shipped form (generated
  tables); partial discriminators and truncated tables are rejected.
- SPEC-013 (safe rules applied): a dead First Aid target's message is
  displayed, a failed Doctor limb awards no experience, the palette fades back
  after every heal, a poison cure removes its pending tick, steal checks always
  use the root victim, a non-positive steal award is skipped, kits refuse
  non-critters with `proto.msg` 582, elevator Cancel changes nothing and keys
  match letters in either case, and the start level follows the player's
  elevation. Hidden loot equipment is a session filter, so no temporary holder
  object is created. Sneak keeps the reference recheck (no draw, for RNG parity).
- SPEC-014 (safe rules applied): an explicit menu selection wins over a
  simultaneous timeout, the editor's Cancel restores character points, a
  four-tag sheet opens with no picks, save/load clicks map rows from y = 87 with
  a fresh double-click window per click, thumbnails fill their unwritten edge,
  the burning-guy preference is read and written under one key (accepting
  `combat_looks`), the monitor stops at the oldest line, the action-menu
  highlight is clamped, and a right-click from use or skill mode returns to
  movement.

## Core contracts

`EntityStore<T>` is a `slotmap::SlotMap<EntityId, T>`. Construct it with
`EntityStore::with_key()`. Removal invalidates a handle even if the slot is reused;
handles are scoped to their owning store. Serde support preserves slot generations
when the store is serialized.

`SequenceIds` owns independent command and event counters. Each begins at 1,
uses checked increments, and returns an error after issuing `u64::MAX`. Persist
the counters with world state (Serde or `from_last`) to avoid reusing IDs after
restore. Allocation does not use clocks, global counters, or randomness.

The engine-owned save envelope has a fixed **24-byte little-endian** header:

| Offset | Size | Meaning |
| --- | --- | --- |
| 0 | 8 | ASCII magic `VAULTSAV` |
| 8 | 2 | Version, currently 1 |
| 10 | 2 | Header size, currently 24 |
| 12 | 4 | Reserved flags, must be zero |
| 16 | 8 | Payload byte length |

`SaveHeader::decode` validates the header prefix. `decode_envelope` also validates
the exact payload length, rejecting truncation and trailing bytes without
allocating from the declared length. Unknown versions, header sizes, and flags
are rejected. Payload encoding and integrity checks are for future phases; this
envelope does not claim compatibility with legacy game saves.
