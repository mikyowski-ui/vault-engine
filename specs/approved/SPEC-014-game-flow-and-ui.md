# SPEC-014: Game Flow and User Interface: Main Menu, Character Creation, Save/Load, Options, Interface Bar, Cursors, and Endgame

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from the inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes reference revision R1, targeting the 32-bit Windows engine at its fixed 640 by 480, 8-bit palettized display. Evidence is static inspection of program start, the main menu, premade selection, the character editor in creation mode, the save/load screens, the options, pause and preferences windows, the generic dialog box, the interface bar, the mouse cursor module, the endgame slideshow and the credits screen. Art sizes were read from corpus FRM headers, message texts from the English corpus `.msg` files, and color keys were resolved through the corpus `color.pal`. No original executable was run.

This document covers:

- the startup sequence, splash screen and palette fades;
- the main-menu loop (Intro, New Game, Load Game, Credits, Exit) and attract mode;
- premade character selection, the character editor in creation mode, and the hand-off to the starting map;
- the save/load slot screens, quick save and quick load;
- the in-game options menu, pause window, quit confirmation, preferences screen and generic dialog box;
- the bottom interface bar: buttons, message monitor, HP/AC counters, AP pips, active item slot, indicator boxes and combat panel animation;
- mouse cursor shapes and modes: movement hex, action pick, action menu, targeting crosshair, edge scrolling and busy cursors;
- the in-game automap window and the Pip-Boy automap page;
- the player inventory and use-on windows: art, layout, stats panel, rotating figure, action menu, cursor modes and item dragging;
- the endgame slideshow, ending movie and scrolling credits;
- the working map set: how visited maps persist across map changes, saves and loads, and how a revisited map ages;
- when roofs are hidden as the player walks under them and restored when the player walks out.

The reference main menu has **no Options button**: its five buttons are Intro, New Game, Load Game, Credits and Exit. Options and Preferences are reached in game (section 3.17). `[Must-match]`

Relationship to other specifications: SPEC-001 owns palettes, FRM sprites and the cursor-hotspot rule. SPEC-002 owns fonts and `.msg` parsing. SPEC-006 owns `.GCD` sheets, stat formulas and the random generator. SPEC-008 owns save-slot files, the 29792-byte thumbnail and the preference words in saves. SPEC-010 owns sound-effect name resolution and audio. SPEC-011 owns level-up and perks. SPEC-012 owns movie playback. SPEC-013 owns the Skilldex window and skill targeting.

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: user-visible layout, art, text, timing, input mapping, state transition, persistence rule, or random-draw sequence. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, undefined behavior, uninitialized reads, stale state, a lookup aimed at the wrong object or file, a presentation glitch, or a platform-specific facility. A clean-room engine may apply the stated safe rule and should keep every other observable behavior. An engine that wants bit-exact parity may offer a compatibility switch that keeps the reference behavior.

**Conventions.**

| Convention | Meaning |
| --- | --- |
| Coordinates | Pixels, x right and y down; window-relative unless marked "screen". A rectangle is written (x, y) w by h |
| Fonts | Font numbers 100-104 select `font0.aaf` through `font4.aaf`; line heights 12, 10, 28, 20, 26 (SPEC-002) |
| Colors | An RGB555 color key `1024 r + 32 g + b` (5-bit channels) looked up in the color.pal RGB555 table (SPEC-001 §2.1). The palette index that the corpus palette returns is given in brackets, for example key 992 [215] |
| Raw index | A palette index used directly, not a color key |
| Sounds | A bare sound name such as `ib1p1xx1` means `sound/sfx/<name>.acm`, resolved per SPEC-010 |
| Art | Art names are under `art/intrface/` unless a path is given; "list index" is the zero-based position in `art/intrface/intrface.lst` |
| Frame | In modal UI loops, one iteration lasting at least `trunc(1000 / 24)` = 41 milliseconds |
| Fade | A calibrated palette fade, nominally 700 milliseconds (section 3.2) |
| Red button | `lilredup.frm` up / `lilreddn.frm` down, 15 by 16, sounds `ib1p1xx1` on press and `ib1lu1x1` on release |

Named colors used throughout: key 18979 [62] label brown; 14723 [63] pressed-label brown; 992 [215] bright green; 32747 [57] selection yellow; 32328 [145] error orange; 21091 [60] amber; 21204 [85] grey-green; 32767 [220] white; 21140 [6] grey; 1313 [96] dark green; 15845 [80] dark olive; 0 [228] black. `[Must-match]`

Evidence areas in reference revision R1 (source file paths and line ranges are withheld under the clean-room quarantine policy):

| Subject | Reference area inspected |
| --- | --- |
| Startup, menu dispatch, new-game load, death scene, attract mode | Program entry and game initialization modules |
| Main menu | Main-menu module |
| Premade selection | Character-selection module |
| Character editor | Editor module |
| Save/load screens, quick slots, thumbnails | Save/load module and buffer scaler |
| Options, pause, preferences, brightness | Options module and configuration defaults |
| Dialog boxes | Bitmap dialog module |
| Interface bar, counters, item slot, combat panel | Interface module, display monitor, item and combat callers |
| Cursors and action menu | Game-mouse module and low-level mouse repeat |
| Palette fades | Game palette layer, color fade primitive, script fade opcodes and their callers |
| Automap | Automap module, seen-state update, Pip-Boy map page |
| Inventory and use-on windows | Inventory module, item-art scaler, action-menu frame builder |
| Map border and scroll limits | Tile module, map scroll and map load |
| Endgame, credits | Endgame and credits modules, word wrapper |
| Working map set, revisit aging | Map load/save, save-slot copying and game reset |
| Roof hiding | Object placement, tile roof fills and map tile-block load |

### Revision history

- **R1-rev2 (2026-10-19).** Builder clarifications, as follows.
  - HP/AC drum roll (section 3.21): exact step sequence, timing, color segments and the triggers that animate.
  - Knocked-out player handling: game over only at the end of a main-loop pass, so a knockout during combat is recovered.
  - Exact dialog-box art and size for the save/load error boxes and the in-combat Pip-Boy refusal (section 3.13).
  - Palette fades: primitive, calibration caveat, and the list of in-game fade sites (section 3.2).
  - Hover, hold-click action menu and edge-scroll corrections and additions (sections 3.27-3.29).
  - New sections 3.33 Automap Window and 3.34 Player Inventory Window, which state that dragging is press-hold-release and has no two-click mode.
- **R1-rev3 (2026-10-19).** Builder placeholders filled, as follows.
  - Section 3.34: art list indices and window-relative geometry of the inventory and use-on windows, the stats panel and examine text, the rotating figure, and the inventory action menu with its Drop and Unload icons. Loot and barter geometry stays in SPEC-013 §3.16 and §3.18.
  - Section 3.29: exact player-distance test and the map-border rectangle, which in R1 accepts centers in hex columns 44-156 and rows 45-154.
  - Section 3.2: the skill-book fade row points to SPEC-013 §3.25, which now holds the skill-book rules. Explosion art and sounds are in SPEC-013 §3.10.
- **R1-rev4 (2026-10-19).**
  - New section 3.35 Working Map Set, Revisits and Aging covers persistent map state:
    - when the per-map `.SAV` files are cleared, written, skipped and reloaded;
    - what they hold, including map variables;
    - how they travel with save slots;
    - the revisit rules: lock unjamming, healing, and replacing old corpses with remains.
- **R1-rev5 (2026-10-20).** Section 3.27 gains "Clicks while the player is busy": a click is never queued, and the section states which clicks cancel the player's current sequence and which are ignored, in and out of combat. The movement-click rule for `interrupt_walk` now says that it cancels any player sequence, not only a walk.
- **R1-rev6 (2026-10-20).** New section 3.36 Roof Hiding and Restoring: the hex-to-roof-square mapping, the empty test (art index 1), the hide and restore fills, the per-placement rule, map load and leave, and two `[May-improve]` defects (the first-placement read outside the grid, and the elevation-change restore on the wrong elevation). New Fixture G. Section 3.35 step 5 now states that a map with no number is never saved.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Numeric Conventions

UI quantities are signed 32-bit integers; divisions truncate toward zero. Save-file and configuration persistence use the layouts of SPEC-008 and the text format of `fallout.cfg`. `[Must-match]`

### 2.2 Splash Image `.RIX`

Splash files are `art/splash/splash<n>.rix`. The reference skips the first 10 bytes without validating them. The table is one layout rule. `[Must-match]`

| Offset | Length | Representation | Meaning |
| --- | ---: | --- | --- |
| `0x0000` | 4 | ASCII | Signature `RIX3` in the corpus; not checked |
| `0x0004` | 2 | Unsigned 16-bit little-endian | Width, 640 in the corpus; not read |
| `0x0006` | 2 | Unsigned 16-bit little-endian | Height, 480 in the corpus; not read |
| `0x0008` | 2 | Bytes | `AF 00` in the corpus; not read |
| `0x000A` | 768 | 256 RGB triples of 6-bit levels | Splash palette |
| `0x030A` | 307200 | Unsigned 8-bit palette indexes | 640 by 480 pixels, row-major, top row first, uncompressed |

Total corpus length is 307978 bytes. A file that is too short is not validated. `[May-improve]`: reject a splash shorter than 307978 bytes and show none.

### 2.3 Configuration Keys

`fallout.cfg` keys consumed by this document, with built-in defaults (the corpus ships no `fallout.cfg`). The table is exact data. `[Must-match]`

| Section | Key | Type, range | Default | Section of this document |
| --- | --- | --- | --- | --- |
| system | splash | integer | 0 | 3.1 |
| system | free_space | integer, KB | 20480 | 3.1 |
| system | times_run | integer | 0 | 3.1 |
| system | language | text | english | 3.1, 3.30 |
| preferences | game_difficulty | 0-2 | 1 | 3.18 |
| preferences | combat_difficulty | 0-2 | 1 | 3.18 |
| preferences | violence_level | 0-3 | 3 | 3.18 |
| preferences | target_highlight | 0-2 | 2 | 3.18 |
| preferences | running_burning_guy (read) / combat_looks (written) | 0-1 | 1 | 3.18 |
| preferences | combat_messages | 0-1 | 1 | 3.18 |
| preferences | combat_taunts | 0-1 | 1 | 3.18 |
| preferences | language_filter | 0-1 | 0 | 3.18 |
| preferences | running | 0-1 | 0 | 3.18 |
| preferences | subtitles | 0-1 | 0 | 3.18 |
| preferences | item_highlight | 0-1 | 1 | 3.18 |
| preferences | combat_speed | integer 0-50 | 0 | 3.18 |
| preferences | text_base_delay | real 1.0-6.0 | 3.5 | 3.18 |
| preferences | text_line_delay | real 0.0-2.0 | 1.399994 | 3.18 |
| preferences | player_speedup | 0-1 | 0 | 3.18 |
| preferences | brightness | real 1.0-1.17999267578125 | 1.0 | 3.18 |
| preferences | mouse_sensitivity | real 1.0-2.5 | 1.0 | 3.18 |
| sound | master_volume, music_volume, sndfx_volume, speech_volume | integer 0-32767 | 22281 each | 3.18 |

### 2.4 Message Resources

All files are under `text/<language>/game/`. The listed indices are exact data; texts are the English corpus strings. `[Must-match]`

| File | Indices used |
| --- | --- |
| `misc.msg` | 0 "Are you sure you want to quit?"; 4 "Quick load game successfully loaded."; 5 "Quick save game successfully saved."; 7 "Pipboy not available in combat!"; 9-13 menu labels INTRO, NEW GAME, LOAD GAME, CREDITS, EXIT; 14 copyright line; 15 "Action Points"; 16 "Hit Points" |
| `lsgame.msg` | 100-103 screen titles; 104 DONE; 105 CANCEL; 106-108 slot-list error; 109 SAVE GAME; 110 LOAD GAME; 111 "- EMPTY -"; 112 "- CORRUPT SAVE  FILE -"; 113 "- OLD VERSION -"; 114 "Empty."; 115 "Error!"; 116 "Old version."; 117-128 JAN-DEC; 130 DESCRIPTION; 131 overwrite question; 132-133 save error; 134-136 load error; 140 "Game Saved."; 141 "Game Loaded." |
| `options.msg` | 0-4 options buttons; 100 GAME PREFERENCES; 101-122 preference captions and buttons; 201-224 value labels; 300 DONE; 301 PAUSED |
| `dbox.msg` | 100 DONE, 101 YES, 102 NO |
| `editor.msg` | 100-102 DONE / OPTIONS / CANCEL labels; 104 AGE; 107 MALE; 108 FEMALE; 116-158 editor captions and validation; 200-209 stat level words; 300-318 condition and derived labels; 400-406 condition descriptions; 500-511 month names; 600-618 editor options dialog |
| `intrface.msg` | 100 SNEAK, 101 LEVEL, 102 CHEM |
| `map.msg` | Location short names and elevation names (section 3.14) |
| `stat.msg`, `skill.msg`, `trait.msg` | Names and descriptions for the selection screen and editor (SPEC-006) |

## 3. Behavioral Rules & Edge Cases

### 3.1 Startup Sequence

In order: `[Must-match]` unless tagged otherwise.

1. **Single instance.** The reference creates a named system mutex and exits with code 1, showing nothing, if it already exists. `[May-improve]`: enforce or omit single-instance behavior with any host mechanism.
2. **Configuration** is loaded with the defaults of section 2.3.
3. **Free disk space.** If measurable free space is below `free_space` KB, a system error box reports the requirement in megabytes (`free_space / 1024`, one decimal) and the program exits. `[May-improve]`: skip the check.
4. **Archives.** The master and critter archives are opened; failure shows a system error and exits.
5. **Registration counter.** If `times_run` is 0 it becomes 1. If it is 1 through 4, an external registration program is launched and awaited, then the counter is incremented. At 5 or more nothing happens. The counter is written whenever the configuration is saved. `[May-improve]`: keep the counter and launch nothing.
6. **Fade calibration** (section 3.2).
7. **Splash.** Start with s = `splash`. Try up to 10 names `art/splash/splash<s>.rix`, adding 1 to s after each miss and wrapping to 0 when s reaches 10. If one opens: set the display palette to black, draw its pixels to the screen at (0, 0), fade to its palette and store `splash = s + 1` without wrapping. The splash stays visible for the rest of initialization; nothing waits for input. Language subfolders are not consulted for the splash. No splash is shown when no file opens.
8. Subsystems initialize. Script-system initialization sets the game clock to tick 264600, 07:21:00 on 5 December 2161; every full game reset does the same.
9. **Movies** (SPEC-012): `art/cuts/iplogo.mve` with flag `0x01` (fade the splash out first), then `art/cuts/intro.mve` with flags 0. Both play on every start; there is no "intro seen" state. `mplogo.mve` is never played.
10. **Main menu** is built and its loop entered (section 3.4). If building fails the program exits normally.

### 3.2 Palette Fades

**Calibration.** At start the engine times one 60-step fade from the current palette to itself, as M milliseconds. It then sets the step count `n = trunc(60 / (M / 700))`, which is `trunc(42000 / M)`. A measurement above 42000 milliseconds gives n = 0, and every later fade is instant. `[Must-match]` The reference clock has about 10-millisecond resolution, so a fast machine can measure 0. The original division by zero then gives no usable step count, and R1 substitutes 46 milliseconds, which gives n = 913. That substitution is a recovered-source workaround, not established original behavior. `[May-improve]`: skip calibration and fade in a fixed 700 milliseconds, with as many steps as the display can show.

**One fade.** `[Must-match]`

1. Remember whether palette cycling (SPEC-001 §3.2) is enabled, then disable it.
2. For k = 0 through n - 1, install a palette whose 6-bit levels are `old - trunc((old - new) * k / n)` (signed, truncating toward zero). Step 0 therefore installs the old palette unchanged.
3. Install the target palette exactly.
4. Re-enable cycling only if it was enabled before.

- **Source palette.** "Old" is the palette the game last installed through its palette layer: by an earlier fade, an instant palette set, or a cycle update of entries 229-255. It is not read back from the display.
- **Brightness.** Every installed step passes through the current brightness (gamma) table (SPEC-001 §3.1), which cannot change during a fade.
- **Timing.** A fade has no timer: its length is n palette uploads. It lasts about 700 milliseconds only when each upload costs what it cost during calibration.
- **Blocking.** A fade blocks completely. There is no input polling, animation, game-clock advance or background task during it. The one exception is a sound-streaming update at steps where `k mod 128 = 0`, made only while music or speech is enabled.

**Targets.** A fade can target any loaded palette. Those used are: the game palette `color.pal`; black, all levels 0; "white", all levels 63, which shows as a full-screen white flash; the splash `.rix` palette; `art/intrface/death.pal`; and the endgame slide palettes. `[Must-match]`

**In-game fades.** While a map is loaded, only the cases below change the palette. `[Must-match]`

| Trigger | Fade | Afterwards |
| --- | --- | --- |
| First Aid or Doctor on an injured target (SPEC-013) | To black before the roll | Back to the game palette after healing and the map-update scripts. First Aid failure does not fade back (SPEC-013) |
| Reading a skill book out of combat (SPEC-013 §3.25) | To black, after the skill increase | Game time advances, the map-update scripts run, then a fade back to the game palette and the book messages |
| `gfade_out` / `gfade_in` with a nonzero operand (SPEC-005) | To black / to the loaded game palette, using the fade above | As the script decides; `gfade_in` does not reload `color.pal` |
| Game movie with fade flag `0x01` (SPEC-012) | To black before playback | The display is set black after playback; with flag `0x02` `color.pal` is reloaded and faded in |
| End of the game session (quit, death or knockout) | To white | The map is unloaded and the game reset (section 3.5) |
| Help screen, F1 | No fade: black, draw `helpscrn.frm` (list index 297), install `art/intrface/helpscrn.pal`, wait for input, black, reinstall `color.pal`. Cycling is suspended meanwhile | |

No palette fade occurs for these: map changes and exit grids, elevation changes, elevators, entering or leaving the world map or town map, resting and waiting, dialogue and talking heads, barter, inventory, loot, the character screen in game, options, preferences, the Pip-Boy, the automap, the Skilldex, or in-game save and load. Several of these suspend palette cycling while open. A map load freezes the view behind the busy cursor and redraws it when done.

**Library fades.** The general-library `fadein` and `fadeout` (SPEC-005) are timed, not stepped:

- Each pass computes the step from the milliseconds elapsed since the previous pass, and runs full background processing, including mouse and button input, between palette writes.
- They do not suspend palette cycling, so entries 229-255 keep being written at full brightness during and after `fadeout`.
- They do not update the game's remembered palette, so the next game fade starts from a stale source and can jump. `[Must-match]`

`[May-improve]`: have library fades suspend cycling and update the remembered palette.

### 3.3 Main-Menu Window

`[Must-match]`

| Element | Value |
| --- | --- |
| Window | Screen (0, 0) 640 by 480, created hidden; palette `color.pal` |
| Background | `mainmenu.frm` (list index 140), 640 by 480, opaque; the five button plates are part of the art |
| Copyright | `misc.msg` 14, font 100, color key 21204 [85], at (15, 460); clipped at the right edge |
| Version | Text `FALLOUT 1.1`, font 100, same color, right edge at x = 615, y = 460 |
| Button i = 0-4 | Rectangle (425, 45 + 41i) 26 by 26; up `menuup.frm` (299), down `menudown.frm` (300); drawn transparently; only nonzero pixels of the up image are clickable |
| Label i | `misc.msg` 9 + i, font 104, color key 21091 [60], top-left (520 - trunc(w / 2), 46 + 41i), drawn into the background |
| Hotkeys | `i`, `n`, `l`, `c`, `e` in either case for buttons 0-4 |

No hover images exist. Hiding with animation fades to black then hides the window; showing reveals the window, reloads `color.pal` and fades to it.

### 3.4 Main-Menu Loop

At the top of every loop pass, the keyboard buffer is cleared, the menu music `sound/music/07desert.acm` is stopped and restarted from its beginning, looping, and the menu is shown with a fade. The idle timer starts when the loop is entered and is **not** reset by mouse movement or keys. Each poll: `[Must-match]`

1. A button hotkey (either case) or a button's mouse release plays `nmselec1` and selects that button. Credits with either Shift key physically held, by key state, becomes **Quotes**.
2. Otherwise: Ctrl+R starts self-run recording (section 3.6). `+` or `=` raises and `-` or `_` lowers brightness by one step (section 3.18), applies it and saves the configuration immediately. `d` or `D` triggers attract mode. A mouse press on a button plays `nmselec0` (not on auto-repeat), so a click sounds `nmselec0` then `nmselec1`; a hotkey sounds only `nmselec1`.
3. Esc selects Exit with `nmselec1`.
4. A pending "return to main menu" request is cleared. Otherwise, when 120000 milliseconds have elapsed since loop entry, the result is Timeout.

If a selection and the timeout fall on the same poll, the timeout replaces the selection. `[May-improve]`: an explicit selection wins. The timeout is fixed at 120 seconds; no configuration key changes it. `[Must-match]`

The music restart also happens after a no-op choice (Quotes with the language filter on, or a failed demo), producing an audible restart. `[May-improve]`: do not restart a track that is already playing.

### 3.5 Menu Choices and Returns

| Choice | Behavior | Then |
| --- | --- | --- |
| Intro | Fade out and hide the menu; play `art/cuts/intro.mve` with flag `0x08` (pause music) | Loop top |
| New Game | Fade out, hide and destroy the menu; run premade selection (section 3.7). On "start" run the new-game hand-off (section 3.12) and the game loop. Any other result rebuilds the menu | Loop top, after the end-of-game sequence when a game ran |
| Load Game | Section 3.16 "from main menu" | Loop top |
| Credits | Fade out and hide; credits from `text/<language>/credits.txt` in normal style (section 3.32) | Loop top |
| Quotes | Only if `language_filter` was 0 when the program first reached the menu: fade out; credits from `quotes.txt` in reversed style. Otherwise nothing | Loop top |
| Exit | Fade out, destroy the menu, stop music, shut down, save the configuration, exit with code 0. No confirmation | Program ends |
| Timeout, `d` | Attract mode (section 3.6) | Loop top |

The table is exact behavior. `[Must-match]` The language filter is read once per program run for Quotes. `[May-improve]`: read the setting when Quotes is requested.

**End of a game session.** The game loop ends when a quit request is set, or when the player carries the dead or knocked-out status at the end of a main-loop pass; the latter also requests the death scene. `[Must-match]`

**Knocked-out player.** A knockout is game over only when it is still present at the end of a main-loop pass. Combat runs inside a single pass, so during combat the check is not made. `[Must-match]`

- **During combat.** The player's turns are skipped while knocked out (SPEC-007 §3.3). The wake-up event queued at knockout, `10 * (35 - 3 * Endurance)` ticks, still runs, because each round adds 5 game seconds (50 ticks). It clears knockout, leaves the player knocked down and makes the player stand at a turn start (SPEC-007). Combat can also end while the player is still out. Normal combat cleanup then wakes a living prone player at once and removes the pending wake-up event, so the game continues. The exception is a combat ended by a scripted combat-ending actor: cleanup does not wake the player, the knockout survives, and the game ends with the death scene at the end of that pass. A loaded save ends combat without that cleanup and restores the saved status instead.
- **Outside combat.** A knockout applied outside combat, for example by a script, ends the game at the end of the current main-loop pass. Its wake-up event never gets a chance to run.
- Death ends the game the same way, through the same death scene. Neither path shows a separate "game over" dialog. Teardown fades to white, unloads the map and performs a full game reset (reloading `data/vault13.gam`). A quit confirmed in game, in the selection screen or in the editor therefore returns to the main menu, never to the desktop. `[Must-match]`

**Death scene.** `[Must-match]`

1. Show a black screen-sized window with `death.frm` (list index 309, 640 by 480); wait for mouse buttons to be released and flush input. The end-of-session fade left the white palette installed, so the screen is solid white at this point.
2. Load `art/intrface/death.pal` and fade to it, from white.
3. Draw D(1, 4) and play `sound/speech/narrator/nar_<draw + 2>.acm`, one of `nar_3` through `nar_6`.
4. Wait for a key or mouse event, or the end of the speech; if speech failed to start, wait 3000 milliseconds instead.
5. Stop speech and wait for button release; if no input ended the wait, pause 500 milliseconds more.
6. Fade to black, then reload `color.pal` without a fade, so the screen stays black until the menu fades in. Restore the arrow cursor.

### 3.6 Attract Mode and Self-Run Demos

A toggle starting at "demo" flips on every attract trigger. On a demo turn, if at least one `selfrun/*.sdf` file exists, the current demo plays and the demo index advances cyclically. Otherwise, or on the other turn, the menu fades out and `art/cuts/intro.mve` plays with flag `0x08`. The corpus has no demo files, so every trigger plays the intro. A demo that fails to prepare shows nothing but still advances the index and flips the toggle. `[Must-match]`

Demo playback hides and destroys the menu, stops music, seeds the generator with `0xBEEFFEED` (SPEC-006 §3.4), resets the game, loads `premade/combat.gcd`, loads the demo's map by the section 3.12 sequence, plays the recording, then fades to white, unloads, resets and rebuilds the menu. The demo file format is outside this document. `[Must-match]`

Ctrl+R recording is a developer feature: a "Select Map" list of `maps/*.map` at screen (80, 80), then a name prompt at (100, 100) limited to 11 characters, then the same sequence as playback while recording. `[May-improve]`: omit it.

### 3.7 Premade Selection Screen

`[Must-match]`

| Element | Value |
| --- | --- |
| Window | Screen (0, 0) 640 by 480, background raw index 228 |
| Background | `pickchar.frm` (list index 174); all button captions (TAKE, MODIFY, CREATE CHARACTER, BACK, CHARACTER SELECTION) are part of the art |
| Monitor area | (40, 30) to (599, 329), saved at open and restored before each refresh |
| Previous / Next | (292, 320) and (318, 320), 20 by 18; `slu.frm` / `sld.frm` and `sru.frm` / `srd.frm`; sounds `ib2p1xx1` / `ib2lu1x1` |
| Take / Modify / Create / Back | Red buttons at (81, 323), (435, 320), (80, 425), (461, 425); they act as `t`, `m`, `c` and Esc |

The premade list is fixed and cyclic, and starts at entry 0 every time the screen opens:

| Entry | Sheet and bio | Portrait (list index) | ID text |
| ---: | --- | --- | --- |
| 0 | `premade/combat.gcd`, `premade/combat.bio` | `combat.frm` (201) | VID 208-197-88-125 |
| 1 | `premade/stealth.gcd`, `premade/stealth.bio` | `stealth.frm` (202) | VID 208-206-49-229 |
| 2 | `premade/diplomat.gcd`, `premade/diplomat.bio` | `diplomat.frm` (203) | VID 208-206-49-227 |

**Keys.** `t`/`T` takes the current premade and returns "start". `m`/`M` opens the editor in creation mode on it. `c`/`C` resets the player to blank defaults, then opens the editor. Editor Done returns "start"; editor Cancel, or an editor that fails to open, reloads the current premade. `b`/`B`/Esc returns to the menu. Left and Right arrows play `ib2p1xx1` and select the previous or next entry. F10 opens the quit confirmation (section 3.17). `+`/`=` and `-`/`_` change brightness. A pending quit request leaves the screen. The screen fades in from black on entry and to black on exit; menu music continues. `[Must-match]`

**Refresh.** Every refresh reloads the selected sheet into the player (SPEC-006 §2.3), which is why Take needs no extra load and a cancelled edit is undone. Then: `[Must-match]`

- Portrait (212 by 187) drawn with index 0 transparent at (150 - trunc(w / 2), 240 - h) = (44, 53), with **every odd row transparent** (a scan-line effect). The reference achieves this by zeroing rows of the cached art itself. `[May-improve]`: skip odd rows while drawing without modifying shared art.
- ID text: font 101, key 992 [215], centered on x = 150 at y = 252.
- Name: font 101, key 992, centered on x = 318 at y = 40, width limit 160.
- SPECIAL rows at y = 70 + 10k, k = 0-6: "<name> NN" (`stat.msg` 100 + k, two-digit zero-padded effective value) right-aligned to end at x = 348, then two spaces and the level word (`stat.msg` 301 through 310, value clamped to 1-10) from x = 348.
- Then, labels right-aligned to x = 365 and values from x = 365: y = 150 "Hit Points" (`misc.msg` 16) " cur/max"; y = 160 armor class (`stat.msg` 109); y = 170 "Action Points" (`misc.msg` 15); y = 180 melee damage (`stat.msg` 111).
- The first three tag skills at y = 200, 210, 220: name (`skill.msg` 100 + skill) right-aligned to 365, then " NN%". Both traits at y = 230, 240: name (`trait.msg` 100 + trait) right-aligned to 365.
- Bio: text-mode lines of up to 255 bytes, font 101, key 992, at x = 420 from y = 40 stepping 10 **while y < 260** (at most 22 lines), width limit 220. A missing bio draws nothing.

A sheet with fewer than three tags or two traits makes the reference copy a missing name. `[May-improve]`: draw an empty line.

### 3.8 Character Editor: Creation-Mode Layout

On entry the editor snapshots the player (stats, bonuses, current HP, name, perks, unspent skill points, tags, traits, last level). Remaining tag picks = (number of trailing empty slots among the sheet's four tag slots) - 1, which is 3 for a blank sheet and 0 for a premade. Character points are a separate counter: 5 after a reset, otherwise the loaded sheet's character-points field (0 in the premades). The window is screen (0, 0) 640 by 480 with background `edtrcrte.frm` (list index 169). Palette cycling is disabled, the cursor is the arrow, and the selected info line starts at 0 (the Strength card). `[Must-match]`

| Element | Position | Art, text, font, color |
| --- | --- | --- |
| Name button | (9, 0) 145 by 35 | `nameoff.frm` / `nameon.frm`, mask `namemsk.frm`; press sound `ib3p1xx1`, no release sound |
| Age button | (154, 0) 81 by 32 | `ageoff.frm` / `ageon.frm`, mask `agemask.frm`; same sound |
| Sex button | (235, 0) 80 by 32 | `sexoff.frm` / `sexon.frm`, mask `sexmask.frm`; same sound |
| Button captions | Row 6 inside each image | Font 103; key 18979 [62] up, 14723 [63] down. Name at x = 75 - trunc(w / 2); "AGE nn" (`editor.msg` 104) at x = 41 - trunc(w / 2); `editor.msg` 107 or 108 at x = 40 - trunc(w / 2) |
| SPECIAL rows | Y = 37, 70, 103, 136, 169, 202, 235 | |
| SPECIAL number | (58, Y), 28 by 24 | Big number (below); red set when the shown value exceeds 10 |
| SPECIAL word | (103, Y + 8) | Font 101; `editor.msg` 199 + value clamped to 1-10; key 992, or 32747 [57] when selected |
| SPECIAL plus / minus | (149, Y) 16 by 12 / (149, Y + 11) 17 by 12 | `splsoff.frm` / `splson.frm`, `snegoff.frm` / `snegon.frm`; press sound `ib1p1xx1` |
| CHAR POINTS | Caption (14, 286), counter (126, 282) | `editor.msg` 116, font 103, key 18979 |
| Conditions | x = 194, y = 46 + 13k, k = 0-7 | `editor.msg` 300 "Hit Points max/max", then 312-318; active key 992 (selected 32747), inactive key 1313 [96] (selected 15845 [80]) |
| Derived list | Labels x = 194, values x = 288, y = 179 + 13k, k = 0-9 | Armor class 302, action points 301, carry weight 311, melee damage 304, damage resistance 305 "n%", poison resistance 306 "n%", radiation resistance 307 "n%", sequence 308, healing rate 309, critical chance 310 "n%" |
| OPTIONAL TRAITS | (52, 326) | `editor.msg` 139, font 103 |
| Traits | 0-7 at x = 47, 8-15 at x = 199; y = trunc(353 + 13.56k) | Font 101, `trait.msg` 100-115 |
| Trait buttons | x = 23 (left column) or 298 (right), y = 352 + 13k, 17 by 11 | `tgskloff.frm` / `tgsklon.frm`; press `ib1p1xx1` |
| SKILLS | (380, 5) | `editor.msg` 117, font 103 |
| Skills | Names x = 380, values "NN%" x = 573, y = 27 + 11k, k = 0-17 | Font 101 |
| Tag buttons | x = 347, y = 26 + 11k, 17 by 11 | `tgskloff.frm` / `tgsklon.frm`; press `ib1p1xx1` |
| TAG SKILLS | Caption (422, 233), counter (522, 228) | `editor.msg` 138 |
| Info card | Area (345, 267) 277 by 170 | Below |
| OPTIONS, DONE, CANCEL | Red buttons at (343, 454), (455, 454), (552, 454); captions at (363, 454), (475, 454), (571, 454) | `editor.msg` 101, 100, 102, font 103 |

Skill and trait colors: untagged key 992, selected 32747; tagged or picked key 21140 [6], selected 32767 [220]. `[Must-match]`

No optional-traits counter is visible: the reference draws one at (522, 228) and immediately overdraws it with the tag counter. `[Must-match]`

**Big numbers.** `bignum.frm` (list index 170, 336 by 24) holds 14-by-24 cells: 0-9 digits, 11 a rolling transition frame, white set at x offset 0 and red set at offset 168. Two digits are always shown. A value outside 0-99, or a previous value outside 0-99, shows "99". An animated change shows cell 11 in the ones position for 123 milliseconds, then the new ones digit, then does the same for tens if the tens digit changed. `[Must-match]`

**Name caption truncation.** If the name is wider than 100 pixels in font 103, cut it at the first prefix whose width before the current character exceeds 100 (so one glyph may overhang); then, if a space was scanned, cut back to just after the last space. `[Must-match]`

**Info hot regions** (a mouse press selects the info line and redraws the lists and card): `[Must-match]`

| Region | Selection |
| --- | --- |
| (19, 38) 125 by 227 | SPECIAL row i: first i with y in [Y_i - (5 - 0.25i), Y_i + 22 + (5 - 0.25i)] |
| (28, 280) 124 by 32 | Character points |
| (52, 324) 169 by 20 | Optional traits |
| (47, 353) 245 by 100 | Trait k: first row with y in [row y - 4, row y + 10], k at most 7; add 8 when x is at least 169 |
| (191, 41) 122 by 110 | Condition line `trunc((y - 41) / 13)` |
| (191, 175) 122 by 135 | Derived stat `trunc((y - 175) / 13)` |
| (376, 5) 223 by 20 | Skills card |
| (370, 27) 223 by 195 | Skill `trunc((y - 27) * 0.092307694)`, at most 17 |
| (396, 228) 171 by 25 | Tag skills card |

**Info card.** Picture from `art/skilldex/` at (484, 309); title in font 102, raw index 228, at (348, 272); two black rules at y = 300 and 301 from x = 348 to 613; description in font 101, black, from (348, 315) stepping 10, word-wrapped to `136 + max(0, min(150, c) - 8)` where c is the leftmost picture column that contains a pixel whose largest 5-bit channel is below 2. A change of picture or title, other than the first card after opening, plays `isdxxxx1`. `[Must-match]`

### 3.9 Character Editor: Input and Validation

`[Must-match]` unless tagged.

**Done** (button, Return, `d`, `D`; the keys play `ib1p1xx1`) validates in this order, showing a dialog at (192, 126) in key 32328 [145] with `iisxxxx1`:

1. Character points not 0: `editor.msg` 118 / 119.
2. Remaining tag picks above 0: 142 / 143.
3. Any SPECIAL shown value (base + trait adjustment + bonus) above 10: 157 / 158. Only the upper bound is checked.

Name, trait count (0-2) and age are not validated; the default name "None" is accepted.

**Cancel** (button, Esc, pending quit request) restores the snapshot. Character points are not in the snapshot; the selection screen's reload hides this. `[May-improve]`: include character points in the snapshot. Ctrl+Q, Ctrl+X and F10 open the quit confirmation; Yes cancels on the next frame and returns through the selection screen to the menu. `n`, `a`, `s`, `o` (either case) or the matching buttons open the name, age, sex and options dialogs; F12 saves a screenshot.

**SPECIAL plus/minus.** A press steps once. Increment requires character points above 0, shown value below 10, and new base value at most 10; it spends one point. Decrement requires base - 1 of at least 1; it refunds one point, with no upper cap on points. After each step, the number animates, derived stats are recomputed (SPEC-006 §3.1), and the stat's card is selected. Holding repeats: idle 41-millisecond frames, and from the 20th frame steps at 5 per second rising by 1 per step to 24 per second, until release, leaving the button, or a failed step. `[Must-match]` The decrement animation compares against the clamped value although the shown value is unclamped. `[May-improve]`: animate from the previously shown value.

**Tag button k.** A tagged skill is untagged and later tags shift down. Otherwise, if picks remain, k fills the first empty slot of the first three. Otherwise `iisxxxx1` and `editor.msg` 140 / 141. The counter animates and skill values recompute with the tag bonus. A loaded sheet with four tags yields remaining picks -1 (shown 99, and Done passes), and removing its fourth tag clears the wrong slot. `[May-improve]`: clamp remaining picks at 0 and remove the clicked slot.

**Trait button k.** A picked trait is removed. Otherwise, with fewer than two picked, k fills the first empty slot. Otherwise `iisxxxx1` and 148 / 149. Trait effects apply live.

**Exit.** On Done, gender-dependent player art is refreshed and the screen fades to black. On either exit the first three working tags and the trait pair are committed (Cancel then restores the snapshot), the info line resets to 0, the player is healed to maximum HP, and the pending level-up flag is cleared.

### 3.10 Name, Age and Sex Dialogs

All three use `charwin.frm` (139 by 72) as a window at screen y = 0, with `donebox.frm` (108 by 24), a "DONE" caption (`editor.msg` 100, font 103, key 18979) and a red Done button. `[Must-match]`

| Dialog | Window x | Layout |
| --- | ---: | --- |
| Name | 17 | `namebox.frm` (111 by 20) at (13, 13); done box (13, 40), caption (50, 44), button (26, 44) |
| Age | 154 | `agebox.frm` (124 by 29) at (8, 7); big number at (55, 10); right arrow `sru.frm` / `srd.frm` at (105, 13); left arrow `slu.frm` / `sld.frm` at (19, 13); done box (13, 40), caption (50, 44), button (26, 44) |
| Sex | 235 | Male `maleoff.frm` / `maleon.frm` (45 by 43) at (22, 2); Female `femoff.frm` / `femon.frm` (45 by 42) at (71, 3); done box (15, 44), caption (52, 48), button (28, 48) |

**Name.** Field at (23, 19), font 101, key 992 text on raw index 100 background, prefilled with the current name unless it is "None". Maximum 11 characters; accepted key codes 32 through 122; Backspace or Delete removes the last character. The cursor is a solid block the width of `_` minus 4 and the line height minus 2, at the text end, toggling every 3 frames. Return (`ib1p1xx1`) or Done accepts a nonempty result; an empty result or Esc leaves the name unchanged. `[Must-match]`

**Age.** Range 16 through 35. Arrows step on press with `ib2p1xx1` and repeat from the 15th frame at 5 per second rising to 24. Keys `+`, Right and uppercase `N` step up; `-`, Left and uppercase `J` step down. Return or Done keeps the value; Esc or a quit request restores the age on entry. The restore writes the age including elapsed years back as the base, which is zero years at creation. `[May-improve]`: restore the stored base value.

**Sex.** The two buttons form a radio group acting on press, with the current gender preselected; a press applies the gender immediately. Return, Done and Esc all just close, so Esc does not revert. `[Must-match]` The reference attaches the press sound to Done twice instead of to the gender buttons, so the gender buttons are silent and Done has no release sound. `[May-improve]`: give the gender buttons `ib1p1xx1` and Done its normal red-button sounds.

### 3.11 Editor Options Dialog

Window at screen (238, 90), `opbase.frm` (164 by 217). Five buttons 137 by 34 at x = 13, y = 17 + 37i, from `opbtnoff.frm` / `opbtnon.frm` with captions baked in (font 103 on row 7, key 18979 up, 14723 down), press sound `ib3p1xx1`: SAVE (`editor.msg` 600, `s`), LOAD (601, `l`), PRINT TO FILE (602, `p`), ERASE (603, `e`), DONE (604, `d` or Return with `ib1p1xx1`; Esc also closes). `[Must-match]`

- **Save** lists `*.GCD`, opens a file dialog titled "SAVE" at (168, 80) with default name `editor.msg` 617, and appends `.GCD`. An existing file prompts 609 / 610 (Yes/No). Saving commits the working tags and traits, then writes the sheet (SPEC-006 §2.2). Success shows 607 and closes the options dialog. Failure shows the file name followed by "Error saving !", a swapped format. `[May-improve]`: show "Error saving <name>!". A list failure shows 615.
- **Load** resets the player, reads the sheet, zeroes bonus stats, resets player statistics and perks, recomputes, heals and closes the dialog. A read failure restores the snapshot and points and shows `iisxxxx1` with "Error loading <file>!" (612). If the file dialog itself fails, the reference returns without closing the options window. `[May-improve]`: close it.
- **Print to file** (`*.TXT`, default name 616) inverts its existence test: it asks "already exists / overwrite?" when the file does not exist and overwrites an existing file silently. `[May-improve]`: prompt only when the file exists. The printed sheet layout is outside this document.
- **Erase** asks 605 / 606 (Yes/No, key 992, at (169, 126)). Yes resets the player to blank defaults: 5 points, all SPECIAL 5, age 25, male, name "None", no tags or traits, info line 0. The dialog stays open.

### 3.12 New-Game Hand-Off

When selection returns "start": `[Must-match]`

1. The selection screen has faded to black and closed.
2. Play `art/cuts/ovrintro.mve` with flag `0x04` (stop music), subtitled when enabled.
3. Reseed the generator from the ambient, nondeterministic source (SPEC-006 §3.4).
4. Clear the quit request and the death flag; make the player visible and not flat; hide the cursor.
5. Open a black screen-sized window, load `color.pal` and fade to it (the screen stays black).
6. Initialize the map system and show the empty cursor.
7. Load `maps/v13ent.map` by its fixed name; its header supplies entry hex 20100, elevation 0, orientation 0, map script `V13Cave.int` and world-map city number 35.
8. Start that city's music, `sound/music/06vault.acm`.
9. Fade to white, close the black window, reload `color.pal` and fade to it.
10. Enter the main game loop.

Initial state: clock tick 264600 (set by the reset, section 3.1); level 1, experience 0, unspent skill points 0, reputation and karma 0; HP at maximum; campaign globals from `data/vault13.gam`. The engine grants **no** starting items; any starting kit is granted by map scripts. `[Must-match]`

### 3.13 Generic Dialog Box

`[Must-match]`

| Element | Large box | Medium box |
| --- | --- | --- |
| Art | `lgdialog.frm`, 302 by 127 | `medialog.frm`, 215 by 113 |
| Text | Font 101, each line centered: x = trunc((box width - text width) / 2); body lines wider than 276 pixels wrap | Same |
| Vertical layout | Title at `y0 = 27 + 30 - trunc((body lines + 1) * 10 / 2)`, then body lines every 10 pixels | Same rule |

Size: a Yes/No or two-button box is always large. Otherwise large or medium as requested, else large when the widest line exceeds 168 pixels or the box has more than five lines including the title.

- **Done box.** `donebox.frm` opaque at (97, 98); caption `dbox.msg` 100 in font 103, key 18979, at (132, 101); red button at (110, 102).
- **Yes/No box.** One `donebox.frm` opaque at (37, 98) with caption 101 at (72, 101) and a red button at (50, 102). A second `donebox.frm` drawn transparently at (169, 98) with caption 102 at (204, 101) and a red button at (182, 102).
- **Size selection detail.** When neither size is requested, the widest line is measured in the font current at the call, not in font 101. Text is then drawn in font 101, line height 10. Large boxes use 6 layout lines (`y0 = 27 + 30 - trunc((body lines + 1) * 10 / 2)`). Medium boxes use 5 (`y0 = 23 + 25 - trunc((body lines + 1) * 10 / 2)`). Both use a left margin of 29 when centering is disabled. A box is a modal window placed with its top-left at the given screen point. `[Must-match]`
- **Instances.** These boxes resolve as follows. `[Must-match]`

| Box | Screen top-left | Art and size | Text |
| --- | --- | --- | --- |
| Save/load errors: save failure, load failure, corrupt slot, slot-scan error (section 3.15) | (169, 116) | Large requested: `lgdialog.frm`, 302 by 127, single Done button centered at (97, 98) | Title and body key 32328, centered; title at y = 47 with one body line, y = 42 with two; body lines follow every 10 pixels |
| Pip-Boy in combat, `misc.msg` 7 "Pipboy not available in combat!" (section 3.19) | (192, 116) | No size requested, title only. The title is wider than 168 pixels in every shipped font (221 in font 0, 186 in font 101, 209 in font 103), so the box is always large: `lgdialog.frm`, 302 by 127, Done button at (97, 98) | Title key 32328 at x = `trunc((302 - 186) / 2) = 58`, y = 52 |
| Save overwrite and quit confirmations | (169, 131) and (169, 117) | Yes/No, always large | As above |

- **Keys.** Enter means yes or OK and plays `ib1p1xx1`. Esc means no or cancel. `y`/`Y` and `n`/`N` work in Yes/No boxes only. A pending quit request forces yes.

### 3.14 Save/Load Screen: Layout and Navigation

Five modes share one screen. The title is drawn in font 103, key 18979, at (48, 27). `[Must-match]`

| Mode | Title (`lsgame.msg`) | Row caption |
| --- | --- | --- |
| Save | 102 | 109 |
| Pick quick-save slot | 103 | 109 |
| Load in game | 100 | 110 |
| Load from main menu | 100 | 110 |
| Pick quick-load slot | 101 | 110 |

| Element | Value |
| --- | --- |
| Window | Screen (0, 0) 640 by 480, modal, game palette, no fade on open in game |
| Background | `lsgame.frm`, 640 by 480, opaque |
| Done / Cancel | Red buttons at (391, 349) and (495, 349); captions 104 at (410, 348) and 105 at (515, 348), font 103, key 18979 |
| Scroll arrows | Up (35, 58) and down (35, 72), 11 by 14; `uparwoff.frm` / `uparwon.frm`, `dnarwoff.frm` / `dnarwon.frm`; red-button sounds |
| Slot list area | (55, 87) 230 by 353, filled with the color at window pixel (55, 86) before drawing |
| Preview | 224 by 133 at (366, 58) |
| Load-mode cover | `lscover.frm`, 275 by 173, at (340, 39) |
| Info box | (396, 254) 164 by 60, restored from the background on every change, font 101 |

While open (except from the main menu) map background processing is suspended; palette cycling is off and the cursor is the arrow in all modes. All are restored on close.

**Slots.** Exactly 10 slots, SLOT01 through SLOT10, always all visible; the arrows only move the selection. For zero-based slot i, the caption is drawn at (55, 87 + 34i) as `[`, three spaces, the caption, a space, the two-digit slot number, `:`, three spaces and `]`. The second line is at (55, 97 + 34i). The selected slot's caption and normal second line use key 32747 [57]; others use key 992 [215]. `[Must-match]`

| Slot state | Second line | Color |
| --- | --- | --- |
| Occupied | Stored description, verbatim | Caption color |
| Empty (no `SAVE.DAT`) | Seven spaces, then 111 | Caption color |
| Corrupt (header short or signature check fails, SPEC-008 §2.4) | 112 | Key 32328 [145] |
| Old version | One space, then 113 | Key 32328; unreachable in R1 |

The old-version state is unreachable because the header reader reports only short reads and bad signatures. `[May-improve]`: omit it, or add a real version check that uses 113, 116 and 136 + 135.

**Navigation.** The selected slot persists for the session (slot 1 at program start). Up and Down move by one, clamped without wrap; Home and End select slots 1 and 10; PageUp, PageDown and the mouse wheel do nothing. Enter acts as Done and Esc as Cancel. `+`/`=` and `-`/`_` change brightness. Ctrl+Q, Ctrl+X and F10 open the quit confirmation, and accepting it closes the screen as cancelled. `[Must-match]`

A click in the list hot zone selects slot `trunc((mouse y - 79) / 34)` clamped to 0-9. Because 79 is 8 pixels above the first caption, the last 8 pixels of each gap select the next slot. `[May-improve]`: use `trunc((mouse y - 87) / 34)`; captions and description lines select the same slots either way. A click on the previously clicked slot acts as Done with `ib1p1xx1`. The remembered slot is cleared by a free-running 24-frame counter that a click does not restart, so the double-click window varies from 1 to 24 frames. `[May-improve]`: restart a 24-frame window on each click. Arrow, Home and End keys forget the remembered slot. `[Must-match]`

**Held arrows.** The selection moves once immediately. After 13 idle frames it steps every frame at 5 steps per second, rising by 1 per step to 24 per second (each step waits `trunc(1000 / rate)` milliseconds). The loop ends only on the arrow's release or drag-out event; all other input, Esc included, is discarded meanwhile. `[Must-match]`

**Preview.** In save modes an occupied slot shows its stored thumbnail; an empty or corrupt slot shows the freshly captured thumbnail of the current game. In load modes an empty or corrupt slot shows the cover; an occupied slot first restores the cover region from the background and then draws its thumbnail. A failed thumbnail read leaves the previous slot's thumbnail on screen. `[May-improve]`: show the cover in load modes or the current capture in save modes. `[Must-match]` otherwise.

**Thumbnail capture.** On opening a save screen (before its window exists) and at a direct quick save: turn off the in-world cursor, hide the mouse, redraw the map view, restore the mouse, then scale the top 640 by 380 pixels of the game view to 224 by 133. The ratio is `R = trunc(224 * 65536 / 640) = trunc(133 * 65536 / 380) = 22937`. Source column x covers destination columns `trunc(R x / 65536)` up to but excluding `trunc(R (x + 1) / 65536)`, and rows likewise, with no filtering. Destination column 0 therefore takes source column 2, column 1 takes 5, and column 222 takes 637. `[Must-match]` Because R is rounded down, destination column 223 and row 132 are never written, and the buffer is not cleared, so they hold uninitialized bytes. `[May-improve]`: fill them with the nearest written pixel.

**Info box.** `[Must-match]` unless tagged.

- Occupied, all in key 992. Line 1 at (396, 254): the stored character name. Line 2 at (397, 266): `DD MON YYYY   HHMM`, with the month name `lsgame.msg` 116 + month (1-12), and hour `trunc(ticks / 36000) mod 24` and minute `trunc(ticks / 600) mod 60` from the header's game tick.
- Line 3 at (399, 279): the location short name, a space and the elevation name (`map.msg` 200 + 3 × map + elevation, often empty), wrapped to 164 pixels with a 12-pixel line step. Only the first wrapped line is ever visible, because the wrapper terminates the text at the next line start and never restores it. `[May-improve]`: draw up to three wrapped lines, at y = 279, 291 and 303.
- Short names come from a fixed per-map table of 66 entries, `map.msg` index by map number: 100 for maps 0-2, 22, 23, 51-54, 56, 57, 60, 62; 505 for 3-5, 9; 500 for 6, 35; 501 for 7, 8; 503 for 10-12; 507 for 13-15, 55; 116 for 16; 511 for 17, 18, 33, 34, 47; 119 for 19; 120 for 20, 21; 504 for 24; 502 for 25, 26; 509 for 27, 42, 43; 510 for 28, 29, 44-46; 508 for 30-32, 48; 506 for 36-41, 65; 149 for 49, 50, 58, 59, 61, 63; 160 for 64.
- An out-of-range map or elevation number from a corrupt header gives an unchecked lookup. `[May-improve]`: show an empty location.
- Empty: 114 at (404, 262), key 992. Corrupt: 115 at (404, 262), key 32328. Old version: 116 at (400, 262).
- List text is not clipped to the 230-pixel list area and stale pixels can remain beyond x = 285. `[May-improve]`: clip list text at x = 285.

### 3.15 Save/Load Screen: Committing

**Save modes.** `[Must-match]` unless tagged.

1. Done, Enter or double-click on an occupied slot opens a Yes/No box at (169, 131) with title 131 and no body. No returns to the list.
2. An empty or corrupt slot proceeds with no confirmation and an empty prefilled description.
3. Description box: a modal window at screen (169, 116) with `lsgbox.frm` (290 by 85). Captions in font 103, key 18979: 104 at (56, 57), 105 at (181, 57), and 130 centered at y = 7. Red buttons Done at (34, 58) and Cancel at (160, 58). Both buttons are silent, because the reference attaches their sounds only when button creation fails. `[May-improve]`: give them the red-button sounds.
4. Text field: origin (24, 35), font 101, key 992 text on the background color of `lsgbox.frm` pixel (24, 35). It is prefilled with the existing description for an occupied slot. Maximum 29 characters; accepted codes 32 through 122; Backspace or Delete removes the last character, except that a Backspace or Delete as the first key after opening clears the entire prefilled text. The text is drawn with one trailing space. A 4-by-8 block cursor at the end of that space toggles every 3 frames. Enter or Done accepts; Esc or Cancel returns to the list.
5. On accept, store the description (30-byte field, terminator at byte 29) and save (below). Success closes the screen and refreshes the map view. Failure plays `iisxxxx1`, shows a Done box at (169, 116) with title 132 and body 133, rescans the slots and stays open.

**Save procedure.** Show the busy planet cursor and pause background sound. Create the save and slot directories if missing. Back up the slot's existing files to `.BAK`, write the header and payloads (SPEC-008 §2.4-§3.3), then delete the backups on success or restore them on failure. On success print `lsgame.msg` 140 to the message monitor. `[Must-match]`

**Load modes.** `[Must-match]` unless tagged.

- An empty slot does nothing, with no sound.
- A corrupt slot plays `iisxxxx1` and shows title 134 with body 136 ("Save game file is an older version."). `[May-improve]`: use body 135.
- An occupied slot loads. On failure: `iisxxxx1`, title 134 with body 135; then a fresh empty map is set up, a return-to-main-menu request is raised and the screen closes as cancelled.

**Load procedure.** Show the busy cursor; end active combat; reread the slot header; run the restore of SPEC-008 §3.3. The full reset rereads preferences from the in-memory configuration, and the restored save preferences then replace the current ones in memory without writing the file. Finalization starts the map's music, restores the player name, redraws the bar and indicator boxes and the map view, and requests combat continuation for a mid-combat save. Leftover map backups are removed and `lsgame.msg` 141 is printed. An in-game load has no palette fade. `[Must-match]`

**Slot-scan errors.** These play `iisxxxx1`, show a Done box with title 106 and body 107 followed by either 108 or the literal text `SAVEGAME\` depending on the path, then close the screen. `[May-improve]`: always use 106 / 107 / 108.

### 3.16 Quick Save, Quick Load, and Load From the Main Menu

`[Must-match]` unless tagged.

**Hotkeys in game.** Ctrl+S or F4 opens Save, and Ctrl+L or F5 opens Load, each after `ib1p1xx1` and whether or not the interface bar is enabled. F6 is quick save and F7 quick load, each after `ib1p1xx1`.

One session flag records that a quick slot is known; save and load share it. It is cleared at program start and whenever a normal Save or Load screen opens, and set when a pick-quick-slot screen completes successfully.

- **F6, flag clear.** Run the pick-quick-save screen. It behaves as Save, including the overwrite and description prompts. Success prints 140 and `misc.msg` 5 and sets the flag.
- **F6, flag set.** Save directly to the selected slot with no screen, no confirmation and no prompt. The slot's existing description is kept and a fresh thumbnail is captured. Success prints only `misc.msg` 5. Failure plays `iisxxxx1` and shows the 132 / 133 box.
- **F7, flag clear.** Run the pick-quick-load screen. Success prints 141, then `misc.msg` 4, and sets the flag.
- **F7, flag set.** Load the selected slot directly under a full-screen black window (key 0 [228]), which is removed afterward. Success prints only `misc.msg` 4. Failure plays `iisxxxx1` and shows 134 / 135, then a fresh map and a return to the main menu.

After F7 has picked a slot, F6 overwrites that slot silently. On the direct quick-load failure path the reference builds a malformed message-file path; when that load fails it returns without a dialog and leaves the black window up. `[May-improve]`: load the message file normally, always remove the overlay and always show 134 / 135.

**Load from the main menu.**

1. Open a black screen-sized window, fade the menu to black, destroy the menu and stop music.
2. Prepare the game: clear quit and death, make the player visible and not flat, initialize the map system, show the empty cursor.
3. Load `color.pal` and fade to it behind the black window.
4. Run the load screen in main-menu mode, with no background suspension and no thumbnail capture.
5. On a successful load, remove the black window and run the game loop.
6. Then, in every case, including Cancel and errors: fade to white, close the black window if still open, unload the map, reset, run the death scene if flagged, and rebuild the menu.

A cancelled load therefore flashes white before the menu fades in.

### 3.17 Options Menu, Pause Window and Quit Confirmation

**Opening.** In game, Esc, `o` or `O` opens the options menu after `ib1p1xx1`, only while the interface bar is enabled. Ctrl+P opens the pause window after `ib1p1xx1` regardless of the bar. On the world map, `p`, `P`, Ctrl+P and Alt+P open the world-map pause window. F12 saves a screenshot and is not an options shortcut. `[Must-match]`

**Options menu.** `[Must-match]`

| Element | Value |
| --- | --- |
| Window | `opbase.frm`, 164 by 217, modal, at screen (238, 71) = (trunc((640 - 164) / 2), trunc((480 - 217) / 2) - 60) |
| Buttons i = 0-4 | 137 by 34 at (13, 17 + 37i); up image `opbtnoff.frm` with caption in key 18979, down image `opbtnon.frm` with caption in key 14723; captions `options.msg` 0-4 in font 103, centered, at y = 8; sounds `ib3p1xx1` / `ib3lu1x1` |

Map background processing is suspended and the in-world cursor turned off while the window is open. The actions are:

- SAVE GAME opens the save screen and closes options if a save happened.
- LOAD GAME opens the load screen and closes options if a load happened.
- PREFERENCES opens preferences and returns to options.
- EXIT GAME opens the quit confirmation; if accepted, options closes and the game returns to the main menu.
- DONE closes options.

Keys:

- Esc closes options.
- Enter, `o`, `O`, `d` and `D` play `ib1p1xx1` and close.
- `s`/`S` saves and `l`/`L` loads.
- `p`/`P` plays `ib1p1xx1` and opens preferences.
- `e`, `E`, Ctrl+Q, Ctrl+X and F10 open the quit confirmation.
- `+`/`=` and `-`/`_` change brightness.
- F12 saves a screenshot.

**Pause window.** Background `charwin.frm` (139 by 72) with `donebox.frm` drawn transparently at (13, 42). The window is at screen (250, 150) in game, or (185, 180) on the world map. It shows "DONE" (`options.msg` 300) at (52, 45) in font 103, key 18979, and "PAUSED" (301) in font 104, key 18979, at y = 10, x = `2 + trunc((139 - w) / 2)`. A red button sits at (26, 46). Before the window opens, the view is converted to greyscale through the grey lookup table: the whole game window after a redraw with the cursor hidden, or the world-map region (22, 21) 450 by 442. In game, background processing and palette cycling are suspended. The window closes on any key or button event except the brightness keys, which adjust brightness and keep it open; mouse movement does not close it. In game the map is redrawn on close. `[Must-match]`

**Quit confirmation.** A large Yes/No box at screen (169, 117) with title `misc.msg` 0, no body, title color key 32328. The in-world cursor is hidden and the arrow shown, then both are restored. Yes raises a return-to-main-menu request; it never exits the program. In game it is opened by Ctrl+Q, Ctrl+X or F10 after `ib1p1xx1`; it is also reachable from the save and load screens, options, preferences, the selection screen and the editor. `[Must-match]`

### 3.18 Preferences Screen

**Window.** Screen (0, 0) 640 by 480, modal, background `prefscrn.frm`. Title `options.msg` 100 in font 104, key 18979, at (74, 10). Captions in font 103, key 18979. Value labels in font 101, key 18979. `[Must-match]`

| Column | Caption placement | Messages |
| --- | --- | --- |
| Knobs | Centered on x = 99 at y = 48, 125, 203, 286, 363 | 101-105 |
| Switches | x = 206 at y = 49, 116, 181, 247, 313, 380 | 106-111 |
| Sliders | x = 384 at y = 19, 94, 165, 216, 268, 319, 369, 420 | 112-119 |
| Buttons | DEFAULT 120 at (43, 449), DONE `options.msg` 4 at (169, 449), CANCEL 121 at (283, 449) | |
| Checkbox caption | 122 at (405, 72) | |

Art: `prfbknbs.frm` (46 by 188) holds four stacked 46-by-47 knob frames, frame = value. `prflknbs.frm` (22 by 50) holds two stacked 22-by-25 switch frames. Sliders use `prfsldof.frm` (idle) and `prfsldon.frm` (dragging), both 21 by 12. The checkbox uses `prfxout.frm` (off) and `prfxin.frm` (on), both 18 by 18. The cover is `prefcvr.frm` (207 by 89). Buttons are red buttons. `[Must-match]`

When campaign global 603 is 0, its initial value, `prefcvr.frm` is drawn opaque at (0, 356). The fifth knob row (caption 105) is then neither drawn nor clickable. `[Must-match]`

**Controls.** The table is exact data. `[Must-match]`

| Preference | Control and position | Values: index, label message |
| --- | --- | --- |
| Game difficulty | 3-position knob at (76, 71) | 0 203, 1 204, 2 205 |
| Combat difficulty | 3-position knob at (76, 149) | 0 206, 1 204, 2 208 |
| Violence level | 4-position knob at (76, 226) | 0 214, 1 215, 2 204, 3 216 |
| Target highlight | 3-position knob at (76, 309) | 0 202, 1 201, 2 213 |
| Running burning guy | 2-position knob at (76, 387) | 0 202, 1 201 |
| Combat messages | Switch at (299, 74) | Left 211 "Verbose" = value 1; right 212 "Brief" = value 0 |
| Combat taunts | Switch at (299, 141) | 202 / 201 |
| Language filter | Switch at (299, 207) | 202 / 201 |
| Running | Switch at (299, 271) | 209 / 219 |
| Subtitles | Switch at (299, 338) | 202 / 201 |
| Item highlight | Switch at (299, 404) | 202 / 201 |
| Combat speed | Slider, knob y = 50 | 207, 210 |
| Text delay | Slider, knob y = 125, reversed | 217, 209, 218 |
| Master / music / effects / speech volume | Sliders, knob y = 196, 247, 298, 349 | 202, 221, 209, 222 |
| Brightness | Slider, knob y = 400 | 207, 223 |
| Mouse sensitivity | Slider, knob y = 451 | 207, 218 |
| Affect player speed | Checkbox at (383, 68) | |

Messages 123, 220 and 224 are unused.

**Knobs.** Each row restores (23, Y) 160 by 54 with Y = 66, 143, 222, 304, 382, then draws its frame transparently at the knob position (kx, ky). `[Must-match]`

- Label anchors: index 0 has its right edge at kx + 2, y = ky + 10. Index 1 is centered on kx + 25, y = ky - 4. Index 2 starts at kx + 46, y = ky + 10. Index 3 starts at kx + 46, y = ky + 31. A label containing a space is split at the first space onto two lines 10 pixels apart.
- The click rectangle runs from the left edge of label 0 to the right edge of the last label, using the full unsplit label width, and from y = ky - 4 with height 48. It is evaluated on mouse release:
  1. Within 16 pixels of (kx + 23, ky + 21): step the value ping-pong, upward to the top value, then downward to 0, and so on. The direction is remembered per control and reset to upward at game start or reset.
  2. Otherwise, when y is greater than ky and in [ky + 10, ky + 20]: x from the left edge to kx selects 0 (direction up). For a knob with three or more values, x from kx + 46 to the right edge selects 2 (direction up).
  3. Otherwise, when y is at most ky and x is in [kx + 9, kx + 37]: select 1. The direction becomes down when the old value was nonzero, else up.
  4. For a 4-value knob, y in [ky + 31, ky + 51] with x from kx + 46 to the right edge selects 3 (direction down).
- On change: play `ib3p1xx1`, block 70 milliseconds, play `ib3lu1x1`, then redraw.

**Switches.** Each row restores (251, Y) 113 by 34 with Y = 66, 133, 200, 264, 331, 397, then draws the frame at (299, ky). The frame equals the value, except Combat messages, whose frame is the value XOR 1. `[Must-match]`

- Labels are at y = ky - 5. The left label's right edge is at kx + 4; the right label starts at kx + 21.
- The click rectangle runs from the left label's left edge to the right label's right edge, from y = ky - 5 with height 28. It is evaluated on release:
  1. Within 10 pixels of (kx + 11, ky + 12): toggle.
  2. Otherwise, when y is in [ky - 5, ky + 7]: x from the left edge to kx selects the left option; x from kx + 22 to the right edge selects the right option.
- On change: play `ib2p1xx1`, block 70 milliseconds, play `ib2lu1x1`, then redraw.

**Sliders.** `[Must-match]`

- **Position.** The knob's left edge travels x = 384 through 603. For value v in [min, max] it is `trunc((v - min) * 219 / (max - min) + 384)`. Text delay is reversed: `trunc((6 - delay) * 43.8 + 384)`, so 3.5 sits at x = 493.
- **Labels.** The label strip is at y = ky - 12. A 2-label slider puts label 0 at x = 384 and right-aligns label 1 at x = 624. A 3-label slider puts label 0 at 384, label 1 at `504 - trunc(w / 2) - 2` and right-aligns label 2 at 624. A 4-label slider puts label 0 at 384, label 1 at `444 + trunc(w / 2) - 8`, label 2 at `564 - w - 4` and right-aligns label 3 at 624.
- **Starting a drag.** The hot rectangle is (384, ky - 12) 240 by 23, and a **press** starts a modal drag loop that ignores other input. The press plays `ib1p1xx1` and draws the active knob.
- **Each drag iteration** lasts at least 35 milliseconds:
  1. If the button is up, play `ib1lu1x1`, redraw the idle knob and stop.
  2. Otherwise follow the mouse with hysteresis. When mouse x is at least pos + 14, pos becomes mouse x - 6, at most 603. When mouse x is below pos + 6, pos becomes mouse x - 6, at least 384.
  3. The value becomes `(pos - 384) * (max - min) / 219 + min`, truncated for integer preferences.
- **Release before movement.** Because the release test precedes the movement, a click that releases within the first iteration changes nothing.
- **Live effects.** Volumes apply to the mixer and brightness applies its gamma immediately during the drag. Text delay is stored as it changes, and mouse sensitivity takes effect on release. The effects slider plays `butin1` on the first iteration and every 8th after it; the speech slider plays `sound/speech/narrator/options.acm` on the first and every 41st.
- **Redraw per iteration.** Volume sliders restore their 240-by-24 strip and labels. The others restore only the 240-by-12 knob row.

**Brightness.** Each 6-bit level i is displayed as `min(63, i ^ brightness)`. Hotkeys `+`/`=` and `-`/`_` (in game, in options, in preferences, in the selection screen, the menu and the save/load screens) add or subtract 0.01124954223632812 within 1.0 through 1.17999267578125 (16 steps), apply it, store it and **save `fallout.cfg` immediately**. Inside preferences the slider is not redrawn after a hotkey, and Cancel restores the old gamma while the file keeps the hotkey value. `[May-improve]`: redraw the slider and include the hotkey change in the cancellable state. F2 and F3 in game change the mixer master volume by -2047 and +2047 without updating the preference, so the next preference application resets it. `[Must-match]`

**Checkbox and buttons.** The checkbox toggles `player_speedup` and plays `ib2p1xx1` on both press and release. `[May-improve]`: play `ib2lu1x1` on release. In combat each walking critter animates at its FRM rate plus `combat_speed`; the player only when `player_speedup` is 1 (SPEC-001 §3.4). DEFAULT at (23, 450) resets every preference to its default, including game difficulty, and applies volumes, gamma, mouse and text delay immediately. DONE at (148, 450), also Enter, `p` or `P` with `ib1p1xx1`, keeps the values. CANCEL at (263, 450), also Esc, discards them. Ctrl+Q, Ctrl+X and F10 open the quit confirmation, and accepting it acts as Cancel. `[Must-match]`

**Persistence.** On open, every value is backed up. Cancel or quit restores the backup and reapplies it. Then, if anything was marked changed, every value is written to the in-memory configuration, `fallout.cfg` is saved, all values are reapplied and the target highlight is refreshed. Any click inside a knob or switch rectangle marks "changed", even in a dead zone. Reapplying sets text delays (line delay `(base - 1) / 5 * 2`, clamped to 0-2), item highlight, reloads AI combat messages and drops cached script messages for the filter, and sets volumes, mouse sensitivity and gamma. Gameplay code reads difficulty, violence, highlight, messages, taunts, filter, running, subtitles, item highlight, combat speed and player speedup when it needs them. The burning-guy preference is stored but not read by gameplay in R1. Every value is clamped to its range on application. Saves store the 20 preference words (SPEC-008), and loading a save replaces the in-memory values without writing the file. `[Must-match]` The burning-guy value is read from `running_burning_guy` but written to `combat_looks`, so it never survives a restart through the configuration file. `[May-improve]`: read and write `running_burning_guy`, accepting `combat_looks` on read.

### 3.19 Interface Bar: Window, Buttons and Hotkeys

**Window.** Screen (0, 379) 640 by 100 (`480 - 100 - 1`), created hidden. Screen row 479 is not covered, and rows 379-380 overlap the 640-by-381 map window. Background `iface.frm` (list index 16), opaque. The bar saves four words: enabled, hidden, active hand (0 left, 1 right) and whether the combat panel is open; on load the panel state is restored without animation. `[Must-match]`

| Button | Rectangle | Up / down art | Notes | Sends |
| --- | --- | --- | --- | --- |
| Inventory | (211, 41) 32 by 21 | `invbutup.frm` / `invbutdn.frm` | | `i` |
| Options | (210, 62) 34 by 34 | `optiup.frm` / `optidn.frm` | | `o` |
| Skilldex | (523, 7) 22 by 21 | `bigredup.frm` / `bigreddn.frm` | Transparent, mask `bigredup.frm` | `s` |
| Map | (526, 40) 41 by 19 | `mapup.frm` / `mapdn.frm` | Transparent, mask `mapup.frm` | Tab |
| Character | (526, 59) 41 by 19 | `chaup.frm` / `chadn.frm` | Mask `mapup.frm` | `c` |
| Pip-Boy | (526, 78) 41 by 19 | `pipup.frm` / `pipdn.frm` | Mask `mapup.frm` | `p` |
| Swap hands | (218, 6) 22 by 21 | `bigredup.frm` / `bigreddn.frm` | Transparent, mask `bigredup.frm` | `b` |
| Item slot | (267, 26) 188 by 67 | Composed (section 3.23); disabled `saturbup.frm` | Large sounds `ib3p1xx1` / `ib3lu1x1` | Left: use item; right: `n` |
| End turn | (590, 43) 38 by 22 | `endturnu.frm` / `endturnd.frm` | Exists only while the combat panel is open; disabled art = up art | Space |
| End combat | (590, 65) 38 by 22 | `endcmbtu.frm` / `endcmbtd.frm` | Same | Enter |

Buttons send their key on mouse release. Unless stated, buttons use `ib2p1xx1` on press and `ib2lu1x1` on release. `[Must-match]`

**Game-loop hotkeys** (most only while the bar is enabled; each plays `ib1p1xx1` before acting, so a bar-button click sounds its own pair followed by `ib1p1xx1`). `[Must-match]`

| Key | Action |
| --- | --- |
| `i` | Inventory |
| `o`, Esc | Options |
| `s` | Skilldex (SPEC-013) |
| `c` | Character screen |
| `b` | Swap hands with animation |
| `n` | Cycle item mode |
| Tab | Automap, unless Alt is held |
| `p` / `z` | Pip-Boy / Pip-Boy alarm-clock page; in combat play `iisxxxx1` and show `misc.msg` 7 in a dialog at (192, 116), key 32328 |
| `m` | Cycle cursor mode (as right-click) |
| `a` | Start combat when not in combat |
| `1`-`8` | Skills (SPEC-013 §3.1) |
| Home | Center on the player |
| `,` / `.` | Rotate the player |
| `-`, `=` | Brightness |
| `/` | Print date and time to the monitor, month name `editor.msg` 500 + month - 1 |
| Ctrl+Q, Ctrl+X, F10 | Quit confirmation |
| Ctrl+P | Pause |

In the combat input loop, Space ends the turn, Enter requests the end of combat, and the loop also ends when the player's AP reaches 0. `[Must-match]`

### 3.20 Message Monitor

`[Must-match]` unless tagged.

- Area (23, 24) 167 by 60 in the bar; its background is saved once from `iface.frm` and restored before each redraw. Font 101 (line height 10), so **6 lines** are visible; all text key 992 [215].
- Storage is a ring of 100 lines of up to 79 characters. Each new message begins with the bullet character `0x95` (5 pixels plus 1 spacing). Wrapping: a chunk fits while its width is **less than** `167 - 6 - bullet width`, which is 155 pixels for the first line and 161 for continuation lines without a bullet; breaks occur at the last space. A word too long for a line is stored truncated and clipped at 167 pixels.
- Visible lines are drawn oldest at top, line k (k = 0-5) at x = 23 + k: each line one pixel further right.
- A new message snaps the view to the newest line. Out of combat it plays `monitor` at most once per 500 milliseconds.
- Two invisible buttons scroll: the upper half (23, 24) 167 by 30 scrolls one line older with cursor `suparrow.frm`; the lower half (23, 54) 167 by 30 scrolls one line newer with `sdnarrow.frm`; leaving either restores the arrow. Scrolling newer stops at the newest line. Scrolling older stops at the oldest ring slot, so early in a game the view can scroll into empty lines. `[May-improve]`: stop at the oldest nonempty line.

### 3.21 HP and AC Counters

`numbers.frm` (360 by 17) holds three 120-pixel color bands: white at x = 0, yellow at 120, red at 240. Within a band, digits 0-9 are 9 pixels wide at x = 9d. The "roll down" glyph is at 90, "roll up" at 99, minus (6 pixels wide) at 108 and the plus/blank sign (6 pixels) at 114. A counter at (x, y) is 33 by 17: sign at x, hundreds at x + 6, tens at x + 15, ones at x + 24. Values clamp to -999 through 999. HP is at (473, 40); AC is at (473, 75), always white. `[Must-match]`

HP band: red when HP is below `trunc(max HP * 0.25)`, yellow when below `trunc(max HP * 0.5)`, otherwise white. `[Must-match]`

**Unanimated update.** Draw the new value's sign and digits at once. A digit cell shows the magnitude digit, `trunc(|v| / 100)`, `trunc(|v| / 10) mod 10` and `|v| mod 10`; the sign cell shows plus/blank for v at least 0 and minus otherwise. `[Must-match]`

**Animated roll.** The counter moves one unit at a time, like a mechanical drum, from the previously shown value P to the new value V (V clamped first). `[Must-match]`

- **Step delay.** `t = trunc(250 / (|P - V| + 1))` milliseconds, computed once from the whole change. Each "wait" below is t milliseconds. During a wait the pointer and in-world cursor keep updating, but the caller is blocked: game input is not processed until the roll ends.
- **Direction.** Each unit changes P by +1 or -1 toward V. The digit cells hold the magnitude. Each unit adds +1 to the ones digit when the magnitude grows, or -1 when it shrinks, with decimal carry and borrow. While all three magnitude digits are 0 the step is always +1, so a roll through zero continues outward on the other side of 0. The hundreds digit wraps from 10 to 0 and from -1 to 9.
- **One unit**, with each glyph drawn over the cell before its wait:
  1. Show the "roll up" glyph in the ones cell, then wait.
  2. When the ones digit carries or borrows, show "roll up" in the tens cell and wait. When the tens digit then also carries or borrows, show "roll up" in the hundreds cell and wait, then "roll down" in the hundreds cell and wait. Then show "roll down" in the tens cell and wait.
  3. Show "roll down" in the ones cell, then wait.
  4. Redraw all three digits and the sign of the new P, then wait.
- A unit therefore lasts 3t, plus 2t for a tens carry, plus 2t more for a hundreds carry. The glyph order is the same whether the value rises or falls.
- **HP color segments.** Let r = `trunc(max HP * 0.25)` and y = `trunc(max HP * 0.5)`. When the band color changes, the roll is split into segments at the thresholds it crosses, all using the same t:
  - rising past r, a segment starts at r in yellow; rising past y, a segment starts at y in white;
  - falling past y, a segment starts at y in yellow; falling past r, a segment starts at r in red;
  - the first segment uses the previously shown color.
  When the color is unchanged, there is one segment.
- **First frame.** The roll begins by drawing P's sign and digits. AC is always white.

**When counters animate.** `[Must-match]`

| Counter | Animated | Unanimated |
| --- | --- | --- |
| HP | Combat damage to the player applied through an animated attack; healing from First Aid or Doctor (SPEC-013); using a healing item or drug on the player in the inventory or from the item slot; a drug or stat effect that changes current HP | Explosion damage, which is applied without animation; interface initialization, map load, world-map travel, rest, character editor, direct stat assignment |
| AC | Start of the player's combat turn; end of the player's turn; end of combat; changing worn armor in the inventory | Interface initialization and refresh |

The remembered previous value starts at 0 for both counters and is updated after every draw, animated or not. The first draw after start is unanimated. `[Must-match]` An animated first frame draws the previous value's digits without taking the absolute value, so a negative previous value reads outside the band. `[May-improve]`: always draw digits of the absolute value with a separate sign.

### 3.22 AP Pips

`[Must-match]`

- The strip is (316, 14) 90 by 5. Its clean background is saved at start and restored before each draw. Pip i (0-9) is drawn 5 by 5 at x = 316 + 9i, using `hlgrn.frm` (green), `hlred.frm` (red) or `hlyel.frm` (yellow).
- A normal value draws that many green pips, clamped to 0-10.
- "Not your turn" (the player's turn ended, or another critter is acting) draws all 10 red.
- When combat ends, AP is reset to maximum but the strip is drawn empty, and it stays empty out of combat. When combat begins nothing is drawn, so if enemies act first the strip keeps its previous state until the player's first turn.
- The yellow art is loaded but never drawn. Bonus Move free movement is **not** shown on the pips; it appears only in the movement cursor's AP number (section 3.27).
- Reload, use and use-on from the bar redraw the pips.

### 3.23 Active Item Slot

**State per hand:** item, disabled flag, weapon flag, primary and secondary hit modes, current action and inventory art. The same object keeps its chosen action. A new object resets the action to primary attack for a weapon or "use" otherwise. An empty hand is a weapon with punch as both primary and secondary. Both hands are marked changed at start. A weapon is disabled when both arms are crippled, or one arm is crippled and the weapon is two-handed; a disabled slot shows only the disabled background and the item art, and its button is disabled. `[Must-match]`

**Composition** into 188-by-67 up and down images, starting from `sattkbup.frm` and `sattkbdn.frm`: `[Must-match]`

1. **Action label**, right edge at x = 181. The up image draws it at (181 - w, 7). The down image draws a copy darkened to light level 59641/65536 at (182 - w, 5).

   | Action | Label art |
   | --- | --- |
   | Non-weapon used on a target | `useon.frm` |
   | Non-weapon that is only used | `uset.frm` |
   | Weapon reload | `reload.frm` |
   | Weapon attack animation: punch | `punch.frm` |
   | Kick | `unarmed.frm` |
   | Throw | `throw.frm` |
   | Thrust | `thrust.frm` |
   | Swing | `swing.frm` |
   | Single | `single.frm` |
   | Burst or continuous | `burst.frm` |

2. **Aimed marker** `bullseye.frm` (18 by 18): up image at (163, 42), down image at (164, 40).
3. **AP cost.** Label `mvepnt.frm` (17 by 12) at (7, 48) up and (8, 46) down. One digit from `mvenum.frm` (cells 10 pixels wide, digit d at x = 10d) at (31, 48) up and (32, 46) down. The cost is the weapon's AP cost for attacks (aimed when the marker shows), 2 for reload, and 2 for a non-weapon use or use-on. The cost block is drawn only for costs 0 through 9.
4. **No cost.** When the cost is negative or 10 or more, both images are replaced by `saturbup.frm`, wiping the labels; the button stays enabled unless the weapon is disabled.
5. **Item art.** The item's inventory art, unscaled and centered: up at (trunc((188 - w) / 2), trunc((67 - h) / 2)). Down, darkened to 63571/65536, at (x + 1, y - 2); when y - 2 is negative, it is drawn at y = 0 with the height reduced by 2. Nothing is drawn for an empty hand.

`[May-improve]` defects in composition:

- An attack animation outside the label list leaves the label uninitialized. Safe rule: draw no label.
- The digit 9 cell reads 10 columns from a 99-pixel-wide image. Safe rule: clip it to 9 columns.
- Item art larger than the slot is drawn unclipped. Safe rule: clip it to the slot.

**Ammo bar.** A 1-pixel-wide vertical bar at x = 463 from y = 26, 70 pixels tall. Its fill is `trunc(current / max * 70)`, rounded down to an even number, from ammunition for a weapon or charges for a miscellaneous item, else 0. From the top, the first `70 - fill` pixels are raw index 14; the remaining pixels alternate raw index 196 and raw index 14, so the bar is striped and fills from the bottom. `[Must-match]`

**Mode cycling** (right-click or `n`, weapons and the empty hand only) runs primary, primary aimed, secondary, secondary aimed, reload, then back to primary, skipping modes that are unavailable. `[Must-match]`

- Aimed modes need called shots allowed. Called shots are refused with Fast Shot, burst or continuous animation, and explosion, fire or EMP damage (SPEC-007 §3.4).
- Secondary modes need a secondary hit mode that is not punch or kick and has an attack type.
- Reload needs current ammunition different from the maximum.
- The slot redraws only when the mode changes.

**Left-click.** `[Must-match]`

- **Reload mode.** Reload repeatedly until a reload fails, which can consume several stacks. Then advance the mode, redraw, and play the weapon's ready sound (SPEC-010). In combat 2 AP are required, checked before reloading, and spent only if something was loaded.
- **Other weapon modes.** Set the system crosshair cursor `crsshair.frm` and the crosshair mode, starting combat if not in combat.
- **Use-on non-weapon.** Set the use crosshair `crossuse.frm` and the use mode.
- **Use-only non-weapon.** Use it immediately; in combat it needs 2 AP.

**Swap hands.**

1. Flip the active hand.
2. Animate putting away the old weapon (with its sound) when it has a weapon animation; the slot redraws midway.
3. Animate taking out the new weapon, or return to the plain standing frame.
4. During the animation the bar and the mouse are disabled and the cursor is `wait2.frm`.
5. Afterwards a crosshair or use mode returns to movement.

When combat ends the hands are refreshed with animation if their items changed. `[Must-match]`

### 3.24 Indicator Boxes

The indicator boxes occupy a separate window at screen (0, 358), height 21, which exists only while the bar is visible and boxes are enabled. Each box is `warnbox.frm` (130 by 21). Labels come from `intrface.msg` 100 SNEAK, 101 LEVEL and 102 CHEM, in font 101: green key 992 for SNEAK and LEVEL, red key 31744 for CHEM. A label is centered horizontally and placed at y = `trunc((24 - 10) / 2)` = 7, using a box height of 24 rather than 21. Boxes always appear in the order CHEM, SNEAK, LEVEL, with at most four slots. The first box has its left 3 pixels cut and is drawn 127 wide at x = 0; box k for k of 1 or more is drawn at x = `130k - 3(k + 1)`. The window width is 127 times the box count. `[Must-match]`

### 3.25 Combat Panel Animation

`endanim.frm` (57 by 58, 5 frames at 5 fps, 200 milliseconds each) is blitted opaque at bar position (580, 38); the redraw rectangle is (580, 38) to (637, 96). `[Must-match]`

- **Open** (entering combat): play `iciboxx1`; show frames 0 through 4, the first at once and then one every 200 milliseconds, busy-waiting with the mouse active; create the End turn and End combat buttons; then disable them (red light). The non-animated open, used when loading, draws only frame 4.
- **Close** (normal combat end, with map scrolling briefly disabled): delete the buttons; play `icibcxx1`; show frames 4 through 0 at 200 milliseconds. The non-animated close draws frame 0, and loading closes without animation.
- **Lights.** The start of the player's turn enables the buttons, draws `endltgrn.frm` (57 by 58) transparently over the panel and plays `icombat2`. The end of the player's turn, or a just-opened panel, disables them, draws `endltred.frm` and plays `icombat1`.

### 3.26 Cursor Shapes

Hotspots follow SPEC-001 §3.5: `(trunc(w / 2) - direction X shift, h - 1 - direction Y shift)`. The values below are computed from the corpus art. `[Must-match]`

| Cursor | Art | Size | Hotspot |
| --- | --- | --- | --- |
| None | `blank.frm` | 1 by 1 | |
| Arrow | `stdarrow.frm` | 18 by 18 | (0, 0) |
| Small up / down | `suparrow.frm` / `sdnarrow.frm` | 11 by 10 | (5, 0) / (5, 9) |
| Scroll NW, N, NE | `scrnwest.frm`, `scrnorth.frm`, `scrneast.frm` | 27 by 28, 31 by 26, 28 by 28 | (0, 0), (15, 0), (27, 0) |
| Scroll E, SE, S | `screast.frm`, `scrseast.frm`, `scrsouth.frm` | 25 by 32, 28 by 28, 32 by 26 | (24, 16), (27, 27), (16, 25) |
| Scroll SW, W | `scrswest.frm`, `scrwest.frm` | 27 by 28, 25 by 32 | (0, 27), (0, 16) |
| Blocked NW, N, NE | `scrnwx.frm`, `scrnx.frm`, `scrnex.frm` | 37 by 38, 31 by 46, 37 by 39 | (0, 0), (15, 0), (36, 0) |
| Blocked E, SE, S | `screx.frm`, `scrsex.frm`, `scrsx.frm` | 40 by 33, 35 by 37, 32 by 41 | (39, 16), (34, 36), (16, 40) |
| Blocked SW, W | `scrswx.frm`, `scrwx.frm` | 34 by 38, 40 by 34 | (0, 37), (0, 16) |
| Crosshair | `crsshair.frm` | 21 by 21 | (10, 10) |
| Plus | `plus.frm` | 17 by 17 | (8, 8) |
| Destroy | `destroy.frm` | 33 by 30 | (0, 0) |
| Use crosshair | `crossuse.frm` | 21 by 21 | (10, 10) |
| Watch | `watch.frm` | 12 by 15 | Per rule |
| Planet (animated) | `wait.frm`, 7 frames at 10 fps | 64 by 64 | (32, 32) |
| Watch (animated) | `wait2.frm`, 6 frames at 10 fps | 12 by 15 | Per rule |

Animated cursors advance one frame every 100 milliseconds and loop. While one is shown the in-world cursor is hidden and map clicks are ignored. The animated watch is used for combat transitions and hand swaps; the planet for map loading and save/load. Setting the current cursor again does nothing, except for the arrow and the animated cursors. `[Must-match]`

### 3.27 In-World Cursor Modes, Hover and Clicks

Over the map window the system cursor is blank and an in-world flat object is drawn instead. Over other windows the in-world cursor is hidden and the arrow is shown, except that a crosshair just set from the item slot stays until the pointer enters the map. Leaving the map window out of combat while in crosshair, use or skill mode resets the mode to movement. `[Must-match]`

| Mode | In-world art |
| --- | --- |
| Movement | `msef003.frm`, a 32-by-16 hex outline in raw indices 135 and 137, plus a translucent red outline enabled only in this mode |
| Arrow | `actarrow.frm` (29 by 23) |
| Crosshair | `acrshair.frm` (21 by 21) |
| Use crosshair and the seven skill modes | `crossuse.frm` |

In movement mode the hex snaps to the hovered hex; a second object for the AP text sits at that hex's screen position plus (16, 15). Other modes follow the pointer by pixel. `[Must-match]`

**Mode cycling.** A right-click in the map area (y at most 379, ignoring held repeats) or `m` selects mode `(current + 1) mod 3` over movement, arrow and crosshair. Out of combat a resulting crosshair becomes movement. In combat crosshair is skipped only when the active hand holds a non-weapon; an empty hand counts as a weapon. Entering crosshair turns hostile outlines on, and leaving it turns them off. `[Must-match]` The same formula applies to use and skill modes by their numeric values: use crosshair goes to arrow; First Aid, Steal and Repair go to crosshair (or movement); Doctor and Traps go to movement; Lockpick and Science go to arrow. `[May-improve]`: a right-click from use or skill mode goes to movement.

**Hover.** Feedback is computed once the pointer has rested on one pixel for at least 250 milliseconds, and again only after it moves; moving resets the art and removes any item outline. `[Must-match]`

- **Rest timer.** It restarts on any one-pixel move of the hotspot, on a mode change, and after the action menu closes. When the in-world cursor is turned back on, the timer is back-dated by 250 milliseconds, so hover is computed on the next idle poll. This happens on entering the map window, after an edge scroll, and when the interface is re-enabled.
- **When hover runs.** Only while the in-world cursor is shown over the map window. It does not run during NPC combat turns, when the game mouse is disabled.
- **Object under the pointer.** No object is found while the pointer is over a visible roof outside the player's see-through cutout. Otherwise the topmost solid object that is not dead or knocked out wins, falling back to the bottom-most object hit.

- **Movement.** Compute a path from the player to the hovered hex (SPEC-007 §3.1).
  - No path, including the player's own hex: a red `X`, key 31744.
  - Out of combat with a path: no text.
  - In combat the cost is path length × 1, × 4 with one crippled leg, or × 8 with both, minus remaining free movement, clamped at 0. A cost up to current AP shows the number in white, key 32767; otherwise a red `X`.
  - The text uses font 101, centered in the 32-by-16 cell from row 3, with a one-pixel black outline.
- **Arrow.** The hovered object selects a primary action, and the cursor becomes `actpick.frm` (69 by 62): the arrow at top-left with a 40-by-40 action icon to its right.
  - Item: use. When the `item_highlight` preference is on (default 1), the item also gets an opaque one-color outline, key 30632. The outline is skipped when the object already has an outline or carries the no-highlight object flag `0x1000`.
  - Player: rotate.
  - Talkable critter: talk out of combat, look in combat.
  - Other critter: use (loot).
  - Usable scenery: use; other scenery and walls: look.
  - **Look-at text.** It prints only in arrow mode, at the moment the hover is computed, for any object type including the player and walls. It runs the object's `look_at_p_proc`. Unless the script overrides, it prints `proto.msg` 490, or 491/492 at random for a dead critter. It works in and out of combat, costs no AP, and prints nothing while the player is dead.
  - **No repeat for the same object.** The print is suppressed when the object equals the last object recorded by a hover. Arrow-mode and crosshair-mode hovers both record it, and the record is never cleared. So moving off an object and back does not print again, and switching from crosshair to arrow over the same critter does not print.
  - When `x + 68` is at least 640, the mirrored `actarrom.frm` is used, with the icon on the left.
  - When `y + 39` is at least 381, the art shifts up by `y + 39 - 381 + 2`.
- **Crosshair.** Over a critter other than the player, the cursor becomes `acttohit.frm` (50 by 21): the crosshair at the left and text in font 101 at (22, 1) with a black outline.
  - The text is `NN%` in white: the hit chance for an uncalled torso shot, even in an aimed mode.
  - It is ` X ` in red when the attack is impossible: dead target, the crippled-arm conditions, not enough AP, out of range, an empty magazine, a blocked ranged shot, or no valid attack (SPEC-007 §3.2).
  - Without a critter under the pointer the plain crosshair is shown.

**Clicks** (left release in the map area, y at most 379). Every left-button map action fires on **release**, at the release position; the press itself does nothing. A release before the 250-millisecond hold pulse is an ordinary click. A right click acts on press. `[Must-match]`

- **Movement.** Walk, or run when the `running` preference is on; Shift inverts the preference. In combat the route is limited to AP plus free movement. With Ctrl held in combat, the current attack's AP cost is subtracted from that budget first, and nothing moves if the result is 0 or less. Out of combat, when `fallout.cfg` [system] `interrupt_walk` is on (default 1), a new walk first cancels the player's current sequence, whatever it is doing ("Clicks while the player is busy", below).
- **Arrow.** Perform the primary action:
  - item: pick up;
  - player: rotate, only when standing;
  - talkable critter: talk out of combat, examine in combat;
  - other critter: loot;
  - scenery: use if usable, else examine;
  - wall: examine.
- **Crosshair.** Attack the critter under the pointer. The hover is then marked done at the click position, so the to-hit text stays as it was until the pointer moves. It is recomputed by force only at the start of each player combat turn and when the in-world cursor is turned back on.
- **Use crosshair.** In combat a non-weapon needs and spends 2 AP; when the AP cannot be paid nothing happens and no message is shown. Otherwise use the active item on the target. The mode returns to movement in every case, even with no target.
- **Skill modes.** As SPEC-013 §3.1.

**Clicks while the player is busy.** `[Must-match]` unless tagged. Busy means that a sequence holding the player is still playing, such as an approach, a reach, a put-away, a take-out or a door use. Nothing is ever queued. A click either cancels the current sequence and starts its own, or does nothing. The rules come from SPEC-007 §3.10.1 ("One sequence per owner").

- **In combat**, the first sequence registered on the player's turn disables the game mouse, keyboard and interface bar. It shows the animated watch until every running sequence has ended. Clicks made meanwhile are discarded (section 3.26).
- **Movement click, out of combat.**
  - With `interrupt_walk` on, the click cancels the player's current sequence and the new walk starts. A reach or container use in progress is abandoned. Its unstarted entries are dropped: the use itself, the weapon take-out and the loot request.
  - A sequence the player is not part of keeps playing. An example is a door's opening animation that the use already started.
  - With `interrupt_walk` off, the new walk cannot register while the player is busy, so the click does nothing.
- **Arrow click or action-menu entry** (pick up, use, loot, talk, skill use).
  - If the player's current art is walk or run, the current sequence is cancelled first. A click during the approach of an earlier action therefore replaces that whole action, not only its movement.
  - In any other pose, such as a reach, put-away, take-out or standing inside a sequence, nothing is cancelled. The new action's first player-owned entry cannot register, so the click does nothing and shows no message.
  - Talk out of combat, at object distance below 9 with an unblocked line, has no approach entry. It consists only of callbacks, so it registers even while the player is busy. Its dialogue request is then made while the earlier animation plays.
- **Player click (rotate)** works only while the player's art is standing (above).

### 3.28 Action Menu

**Hold pulse.** Every poll while the left button is down reports "held". A combined pressed-and-held pulse fires when strictly more than 250 milliseconds have passed since the press, then each time more than 250 milliseconds have passed since the previous pulse. The action menu opens only on that combined pulse, in arrow mode, when an object is under the pointer. If no object is under the pointer, a later pulse opens the menu once the pointer reaches one. In other modes a held left button does nothing. `[Must-match]`

Entries top to bottom:

| Target | Entries |
| --- | --- |
| Item | Use, Look, then Inventory and Skill for containers only, Cancel |
| Critter | First: Rotate for the player; Talk when talkable and not in combat; nothing when talkable in combat; Use when not talkable. Then Look, Inventory, Skill, Cancel |
| Scenery | Use if usable, Look, Inventory, Skill, Cancel |
| Wall | Look, Inventory if the wall is usable, Cancel |

Icons are 40 by 40 normal/highlighted pairs: `canceln.frm` / `cancelh.frm`, `invenn.frm` / `invenh.frm`, `lookn.frm` / `lookh.frm`, `rotaten.frm` / `rotateh.frm`, `talkn.frm` / `talkh.frm`, `usegetn.frm` / `usegeth.frm`, `skilln.frm` / `skillh.frm` (the drop and unload pairs are not used in the world menu). They are composed into `actmenu.frm` (69 by 222): the arrow at top-left and icons stacked 40 pixels apart to its right, the first highlighted. The right-edge flip uses `actarrom.frm` with the same rule as the pick art. When `y + 40 * count - 1` is at least 381, the menu shifts up by that amount minus 381, plus 2. Opening plays `iaccuxx1` and pauses background map processing. `[Must-match]`

Selection: while held, each vertical move of more than 10 pixels from the last anchor moves the highlight one entry and resets the anchor. On release the pointer is warped to (original x, last anchor y), the art reverts, and the entry runs. `[Must-match]`

- Each poll moves the highlight at most one entry, however far the pointer moved: up decrements and down increments, with no sound.
- The release is consumed by the menu and never also triggers the primary action.
- While the menu is open, map background processing is paused. That covers object animation, idle fidgets, critter scripts, timed events, the game clock (normally one tick per 100 milliseconds out of combat), floating text, the game mouse and edge scrolling. Sound keeps playing and keyboard input is discarded. `[Must-match]`

- Inventory: use an inventory item on the target.
- Look: examine.
- Rotate and Talk do what they say.
- Use: use the scenery, loot the critter, or pick up the item.
- Skill: open the Skilldex and use the chosen skill on the target; Sneak toggles.
- Cancel: nothing. A pending quit makes the choice Cancel.

The selection index is not clamped. Dragging past either end keeps the highlight on the edge entry while the index keeps counting, so reversing does not move the highlight back at once, and the release reads outside the list. `[May-improve]`: clamp the index to the list.

### 3.29 Edge Scrolling

A pointer at x = 0, x = 639, y = 0 or y = 479 scrolls the map; corners scroll diagonally. A scroll step happens at most once per 33 milliseconds and moves 32 pixels horizontally and/or 24 vertically. The cursor shows the direction's scroll art, or its blocked variant when the map cannot scroll that way; a throttled tick leaves the cursor unchanged. Leaving the edge restores the previous cursor. Scrolling works while the in-world mouse is disabled if scrolling itself was left enabled. `[Must-match]`

- **Edges are screen pixels.** The hotspot is clamped to 0-639 and 0-479. Row 479 lies over the interface bar and still scrolls south. While edge scrolling is active, hover and window logic are skipped and keyboard events are dropped; mouse button events still pass. Every attempt that is not throttled hides the in-world cursor. `[Must-match]`
- **One combined step.** A diagonal is one step. If the diagonal target is blocked, the whole step is blocked; it does not slide along one axis. Blocked attempts also restart the 33-millisecond timer. `[Must-match]`
- **Blocked test.** The new center is the hex under the current center's screen position plus `(32 dx + 16, 24 dy + 8)`. The step is blocked when any of these holds: `[Must-match]`
  - that point has no hex, or the hex is outside 0-39999;
  - scroll limiting is on (the default) and the player-distance test below refuses the target;
  - the target hex holds a scroll-blocker object (misc PID `0x0500000C`) on the current elevation;
  - the target hex is outside the map border below.

  Map loading switches limiting and scroll-blocker checks off for its duration. Two reference quirks: hex 0 is never treated as holding a scroll blocker, and the parity correction meant for the right border bound adjusts the left bound instead (see the map border). `[May-improve]`: test hex 0 like any other, and use left bound 43 and right bound 155, both in mirrored columns.
- **Player-distance test.** `[Must-match]` Positions are in map-window pixels, as the top-left corner of a hex's 32-by-16 cell. The map window is 640 by 380.
  - A is the target hex's position and P the player's hex position.
  - R is the view reference point: (304, 182), or (272, 182) when the current center hex's column (hex mod 200) is even.
  - With `dx = |Px - Ax|` and `dy = |Py - Ay|`, the target is refused when both of these hold:
    - `dx > |Px - Rx|` or `dy > |Py - Ry|`;
    - `dx >= 480` or `dy >= 400`.

  A step that moves the view toward the player is therefore never refused by this test, even when the player is far off screen.
- **Map border.** `[Must-match]` Every map is 200 by 200 hexes. For a hex h, let column c = h mod 200, row r = trunc(h / 200), and mirrored column x' = 199 - c.
  - The border is computed once at engine start, for the 640-by-380 map window, with the view centered on hex 20100:
    1. The hex under window point (-320, -240) is 12702, and the hex under (-320, 620) is 23538.
    2. Left bound `L = |199 - (23538 mod 200) - 98| + 6 = 43`. Top bound `T = |100 - trunc(12702 / 200)| + 7 = 44`. Here 98 is the mirrored column of hex 20100, made even, and 100 is its row.
    3. Right bound `200 - L - 1 = 156`. Bottom bound `200 - T - 1 = 155`.
    4. An even L would be incremented; 43 stays. The right bound is even, so the reference decrements L to 42.
  - A center is accepted only when `42 < x' < 156` and `44 < r < 155`, that is, columns c = 44-156 and rows r = 45-154. The border does not depend on a map's contents.
  - The border applies to every view-center change. Edge and arrow scrolling, the Home key's hex-by-hex walk toward the player, and dialogue centering also apply the limiting and blocker tests. Map entry and elevation changes skip those two tests but not the border.
  - A refused center leaves the view where it was. A refused map-entry hex fails the map load (SPEC-003). The entry hexes of all 72 corpus maps lie inside the border.
- **Arrow keys** scroll one step per key event through the same 33-millisecond gate. Key auto-repeat starts after more than 500 milliseconds, then repeats every more than 80 milliseconds. Arrow keys work regardless of the edge-scroll enable state, but not while the keyboard is disabled. `[Must-match]`
- **Combat and other windows.** During NPC turns and the player's attack animations the interface is disabled, but edge scrolling stays on: the scroll art temporarily replaces the animated watch, which returns afterwards. Scrolling is off during the combat panel's open and close animations, and during dialogue, inventory, the Pip-Boy, the world map and the action menu. Re-enabling the game mouse always turns scrolling back on. `[Must-match]`
- **Click-to-scroll.** A latent mode exists in which a fresh left press on an edge pixel is flagged; any left release clears the flag before it reaches the map. While flagged, hold pulses and right clicks made with the left button down are ignored. Nothing in R1 turns this mode on (there is no configuration key), so it has no effect, and the ordinary release click is never suppressed. `[Must-match]`

### 3.30 Endgame Slideshow

`endgame_slideshow` only requests the slideshow, which runs at the next script-request pass. Setup stops music, pauses map processing, disables palette cycling and shows the blank cursor, then selects font 101, fades to black and opens a screen-sized window. Subtitles are used when the `subtitles` preference is on, read from `text/<language>/cuts/<narration>.txt`. `[Must-match]`

Slides in order. "Global n" is the zero-based campaign variable n of `data/vault13.gam` (SPEC-006 §2.1). Art is under `art/intrface/` with palette `art/intrface/<art name>.pal`, and narration is `sound/speech/narrator/<name>.acm`. The table is exact data. `[Must-match]`

| Step | Condition (first matching row) | Art | Narration |
| ---: | --- | --- | --- |
| 1 | Desert pan, left to right; global 51 nonzero / otherwise | `dp.frm` | `nar_11` / `nar_10` |
| 2 | Global 13 nonzero | `seq2ad.frm` | `nar_15` |
| | Global 30 nonzero and global 31 = 2 | `seq2b.frm` | `nar_13` |
| | Global 30 nonzero | `seq2ad.frm` | `nar_12` |
| | Otherwise no slide | | |
| 3 | Global 7 nonzero | `seq3c.frm` | `nar_18` |
| | Global 132 nonzero | `seq3a.frm` | `nar_16` |
| | Otherwise no slide | | |
| 4 | Global 12 nonzero | `seq4de.frm` | `nar_23` |
| | Global 604 nonzero, global 26 not 0 or 2 | `seq4de.frm` | `nar_22` |
| | Global 604 nonzero, global 26 is 0 or 2 | `seq4abc.frm` | `nar_21` |
| | Global 604 zero, global 26 not 0 or 2 | `seq4abc.frm` | `nar_20` |
| | Global 604 zero, global 26 is 0 or 2 | `seq4abc.frm` | `nar_19` |
| 5 | Global 15 nonzero | `seq5d.frm` | `nar_27` |
| | (Global 104 not 2, or global 37 nonzero) and global 38 zero | `seq5b.frm` | `nar_25` |
| | Global 104 not 2, or global 37 nonzero (with global 38 nonzero) | No slide | |
| | Otherwise | `seq5a.frm` | `nar_24` |
| 6 | Global 108 = 2 and global 250 nonzero | `seq6b.frm` | `nar_29` |
| | Otherwise | `seq6a.frm` | `nar_28` |
| 7 | Global 14 nonzero | `seq7c.frm` | `nar_34` |
| | Global 606 = 1 | `seq7a.frm` | `nar_32` |
| | Otherwise no slide | | |
| 8 | Global 69 below 2 | `seq8abc.frm` | `nar_37` |
| | (Global 114 nonzero and global 115 below 8) or global 115 below 4 | `seq8abc.frm` | `nar_35` |
| | Otherwise | `seq8abc.frm` | `nar_36` |
| 9 | Desert pan, right to left | `dp.frm` | `nar_40` |

After the sequence, and also when setup failed, global 299 is set to 0. `[Must-match]`

**Still slide.** `[Must-match]`

1. Draw the image, load its palette and narration.
2. Fade in from black and wait 500 milliseconds.
3. Start the narration and subtitles.
4. Redraw the image and subtitles each pass until any key or mouse button event, the narration ends, or the subtitles end. A 3000-millisecond timeout applies only when neither narration nor subtitles loaded. With both present, the slide ends when either finishes first.
5. Stop the narration and clear the subtitles.
6. If no input ended the slide, wait another 500 milliseconds.
7. Fade to black and wait for all mouse buttons to be released.

**Desert pan.** `dp.frm` is 1280 by 480. `[Must-match]` unless tagged.

1. Clear the window to black, load `dp.pal` and set the palette to black instantly. Preload the narration.
2. The pan distance is `D = 1280 - 640 = 640` columns, one column per step, 16 milliseconds per step. If the narration loaded with length L seconds, rounded up, and `1000 L` exceeds `16 * D / 2`, the step delay becomes `trunc((1000 L + 16 * trunc(D / 2)) / D)` milliseconds.
3. The fade zone is `F = trunc(D / 4)` = 160. The palette is scaled by `position / 160` over positions 0-159, by `(160 - (position - 480)) / 160` over positions 480-639, and is full in between.
4. Narration and subtitles start when the position reaches 160 (left to right) or 480 (right to left).
5. Any input aborts the pan and stops the narration. At the end: fade to black, clear, wait for button release. A pan that completes by itself does not stop the narration explicitly.

The fade-out start is computed as `640 - F` instead of `D - F`, which is correct only because D is 640. `[May-improve]`: use `D - F` and skip the pan when D is not positive.

**Subtitles.** `[Must-match]` unless tagged.

- Lines have the form `N:text`; the text after the first colon is kept and the number ignored. Lines without a colon are dropped. At most 50 lines are kept, each read through a 255-byte buffer.
- The time per character is the narration length in whole seconds divided by the total character count, or 0.08 seconds without narration. Each line ends at a cumulative time: `trunc(line length * time per character * 1000)` milliseconds added per line.
- Rendering uses font 101 in white key 32767 on a black box the size of each text line. Text wraps to 540 pixels at spaces or hyphens, mid-word when needed, and each line is centered. With n wrapped lines the block starts at `y = 480 - 10 (n + 1)`, leaving a 10-pixel band at the bottom.
- Rendering the last wrapped line writes one byte past the stored string. `[May-improve]`: never write past the terminator.

**Exit.** Restore `color.pal` and fade to it, restore the arrow cursor, re-enable palette cycling and map processing. `[Must-match]`

### 3.31 Ending Movie

`endgame_movie` runs immediately: `[Must-match]`

1. Stop music, pause the map and fade to black.
2. Start music track `maybe` once, without looping.
3. Wait 3000 milliseconds.
4. Play `art/cuts/walkm.mve` for a male player or `art/cuts/walkw.mve` for a female player, with no fade or music flags, so the music continues (SPEC-012).
5. Run the credits from `credits.txt` in normal style with no background.
6. Stop music and return to the main menu.

Whenever `maybe` ends, a background hook starts `10labone` looping. Neither track is in the corpus.

### 3.32 Scrolling Credits

Shared by the main menu (credits and quotes) and the ending movie. `[Must-match]` unless tagged.

- **Setup.** Load `color.pal`; open a screen-sized black window (key 0 [228]); show the blank cursor; disable palette cycling. The background art argument is always "none" in R1, so the background is black. Then fade in.
- **Line codes**, by first character:
  - `;` comment, skipped;
  - `@` title line;
  - `#` name-font line in grey, key 17969 [201];
  - anything else, a name line.
- **Styles.**
  - Normal: titles in font 103, key 13673 [76]; names in font 104, key 18917 [62].
  - Reversed (quotes): titles in font 104, key 18917; names in font 103, key 13673.
  - The row pitch is the larger of the two line heights, 26.
- **Line layout.**
  - Each line is centered horizontally.
  - A line 640 pixels or wider is skipped.
  - An empty line still occupies a row.
  - The stored line keeps its terminating newline, whose 1-pixel spacing shifts centering left by up to 1 pixel. `[May-improve]`: strip terminators before measuring.
- **Scrolling.**
  - The whole text layer moves up 1 pixel per tick, each tick at least 38 milliseconds after the previous one. New lines enter one pixel row at a time at the bottom.
  - Each tick composites the text transparently over the background.
  - After the last line, 480 blank ticks let the text scroll off.
- **Exit and the easter egg.**
  - Any key or mouse button event ends the credits, except the exact lowercase sequence `b`, `o`, `o`, `m` typed while lines are still scrolling. A wrong character or a click ends them.
  - Completing the sequence, when `violence_level` is not 0, plays `tblcrd.frm` (31 frames, 369 by 279) transparently at (135, 201), one frame every 2 ticks. It can be triggered again.
  - During the trailing blank ticks any input ends the credits.
  - On exit: fade to black, close the window, restore the arrow cursor and re-enable palette cycling.

### 3.33 Automap Window

**Opening.** `[Must-match]`

- **Tab** opens the automap with the scanner off. It works only while the interface bar is enabled and neither Alt key is held, and plays `ib1p1xx1` first. The bar is enabled during the player's own combat turn, so the automap opens in combat too.
- The bar's **Map** button sends Tab, so it follows the same path.
- **Using a Motion Sensor**, item PID 59, from the inventory or the item slot spends one charge and opens the automap with the scanner already on. With no charges left, it prints `item.msg` 5 ("%s has no charges left.") to the monitor and does not open.
- The automap never refuses because of the current map. Maps excluded from the automap database (SPEC-008) still open the window.

**Window.** Screen (75, 0), 519 by 480, modal. Its whole area is covered by `automap.frm` (list index 171, 519 by 480). Font 101; arrow cursor. Background map processing (animation, fidgets, time) is paused while the window is open. `[Must-match]`

| Control | Rectangle | Art | Sounds | Sends |
| --- | --- | --- | --- | --- |
| Scanner | (111, 454) 15 by 16 | Red button | Red-button sounds | `s` on release |
| Cancel | (277, 454) 15 by 16 | Red button | Red-button sounds | Esc on release |
| Detail switch | (457, 340) 42 by 74 | `autoup.frm` (172) up / `autodwn.frm` (173) down; a two-state toggle | `toggle` on press and release | `l` when switched down, `h` when switched up |

The switch starts down when detail is low. Detail (high or low) persists between openings and is saved with the game (SPEC-008), default low. The scanner state is cleared at every opening. `[Must-match]`

**Keys.** `[Must-match]` unless tagged.

- Tab, Esc, `a` or `A` close the window.
- `h`/`H` select high detail, and `l`/`L` low detail. Each redraws only if the setting changes. The keys do not move the switch art. `[May-improve]`: keep the switch in step with the keys.
- `s`/`S` resets the shown elevation to the player's (it never differs in R1). Then, if the scanner is off, it looks for a Motion Sensor in the left hand, then the right hand.
  - Found with charges: spend one charge, turn the scanner on and redraw.
  - Otherwise: play `iisxxxx1` and show a dialog box at screen (165, 140), key 32328. The text is `misc.msg` 17 ("The motion sensor option is not installed.") when no sensor is held, or 18 when the held sensor is empty.
- Ctrl+Q, Alt+X and F10 open the quit confirmation; a confirmed quit closes the window. F12 takes a screenshot.

**Drawing.** The window always shows the player's current elevation. Every object on it with a hex is visited in object-list order, and its color is chosen like this. `[Must-match]`

1. Scanner on, and the object is a critter that is not hidden and not dead: key 31744 (bright red), whether or not it has been seen.
2. Otherwise objects without the seen flag `0x40000000` are skipped. A seen object is drawn:
   - in key 32328 (orange) if it is the exit-grid marker, scenery PID `0x02000031`;
   - in key 992 (bright green) if it is a wall;
   - in key 480 (dark green), with high detail only, if it is scenery other than PID `0x02000158`;
   - in key 31744 if it is the player.
   Any other object is not drawn: items, misc objects, critters without the scanner, and scenery in low detail.

For hex t, let c = `t mod 200` and r = `trunc(t / 200)`. The object paints window pixels (x, y) and (x + 1, y), with `x = 449 - 2c` and `y = 8 + 2r`, so the map is mirrored left to right. A dark-green pixel is not painted over a bright-green one, but a later wall overwrites scenery. The player also paints (x - 1, y), (x, y - 1) and (x, y + 1), a small cross that does not blink. `[Must-match]`

When the map has an index (SPEC-008), two labels are printed in key 992 with a transparent background, width 240: the area short name from `map.msg` at (150, 380) and the elevation name at (150, 396). `[Must-match]`

**Seen state.** `[Must-match]`

- Every hex the player enters sets one bit in a 40000-bit visited set, 8 hexes per byte in hex order, 25 bytes per map row of 200 hexes.
- The set is processed when the automap opens, before an automap-database write, and when the map's objects are saved. Processing marks a window of bytes around every nonzero byte b: offsets `b + 25k + d` for k = -16 through 15 and d = -2 through 2, kept within bytes 0 through 5000. That covers 16 rows before and 15 after, by 40 hexes in 8-hex groups.
- Every object on a hex of a marked byte and on the player's current elevation gets the seen flag permanently, and the flag is saved with the object. The visited set is then cleared.
- Near a row's start or end, d crosses into the neighboring row because bytes run continuously. `[Must-match]`

**Pip-Boy automap page.** This page draws from the saved automap database (SPEC-008), not from live objects. It is current only up to the last database write: leaving the map, saving the game, or taking an elevator. Each stored cell is 2 pixels wide and 1 pixel tall at origin (238, 105) of the Pip-Boy window, wall cells in key 992 and scenery cells in key 480. The player, critters and items are not drawn. Rows from 188 on would fall below the 480-pixel window; R1 relies on those rows being empty. `[Must-match]` `[May-improve]`: clip cells to the window.

### 3.34 Player Inventory Window: Cursor Modes and Dragging

The inventory window is at screen (80, 0); the rectangles below are screen coordinates, inclusive. `[Must-match]` unless tagged.

**Window art and layout.** Both windows open at screen (80, 0) with the window's art as background. Art is given as name and `intrface.lst` index. The loot and barter windows are in SPEC-013 §3.16 and §3.18.

| Element | Inventory window | Use-on window |
| --- | --- | --- |
| Background | `invbox.frm` (48), 499 by 377 | `use.frm` (113), 292 by 376 |
| Item list | 6 rows; buttons (124, 35 + 48i) 64 by 48; item art scaled into 56 by 40 at (128, 39 + 48i) | Same |
| Quantity label | As in the loot window (SPEC-013 §3.16): font 101, white, at the icon's top-left | Same |
| Scroll up | (208, 39) 22 by 23, `invupout.frm` (49) up, `invupin.frm` (50) down; key Up | Same |
| Scroll down | (208, 62) 22 by 23, `invdnout.frm` (51) up, `invdnin.frm` (52) down; key Down | Same |
| Done / Cancel | Done: red button (`lilredup.frm` 8, `lilreddn.frm` 9) at (517, 329); sends Esc | Cancel: red button at (313, 328); sends Esc |
| Rotating figure | 60 by 100 box at (256, 37); an invisible button over it goes up one container level | Same |
| Armor slot | (234, 183) 90 by 61 | None |
| Left hand (item 1) / right hand (item 2) | (234, 286) / (325, 286), each 90 by 61 | None |
| Stats panel | (377, 44) 152 by 188 | None |

- The scroll buttons use the red-button sounds and move one row: up needs a positive offset, and down needs offset + 6 below the entry count.
- The inventory window's keys are Esc, Up and Down, plus Ctrl+Q and Ctrl+X for the quit confirmation. The use-on window also takes Home (offset 0), End (offset: the entry count minus 6, at least 0), PgUp and PgDn (6 rows).
  - PgDn always redraws. When the new offset + 6 would reach the entry count, it is clamped to the entry count minus 6, at least 0.
  - PgUp redraws only when it clamps to 0. Otherwise the offset changes but the list keeps showing the old rows until the next redraw. `[May-improve]`: redraw after every PgUp.
- Slot art is scaled into the slot's 90 by 61 rectangle. An item held in both hands is drawn once, scaled into 180 by 61 at the **right-hand** position (325, 286), so it extends past the hand area. Before redrawing the hands in that state the reference pastes `sattkbup.frm` (32, 188 by 67) at (232, 284) instead of the window background. `[May-improve]`: draw a both-hands item at (234, 286) and restore the background from `invbox.frm`.

**Stats panel** (inventory window only). Font 101, bright green (key 992), line height 10, drawn over a fresh copy of the panel's `invbox.frm` background. Coordinates are screen coordinates. `[Must-match]`

| Item | Position | Content |
| --- | --- | --- |
| Name | (377, 44), clip 80 | The inventory owner's name |
| Rule | (377, 59) to (520, 59) | One-pixel line |
| Rows k = 0-6, left column | Label (377, 64 + 10k), value (401, 64 + 10k) | `inventry.msg` k (ST, PE, EN, CH, IN, AG, LK) and the effective primary stat |
| Rows k = 0-6, right column | Label (417, 64 + 10k), value (481, 64 + 10k) | `inventry.msg` 7 + k. Row 0 `current/max` HP; row 1 three spaces then AC; rows 2-6 `DT/DR%` for normal, laser, fire, plasma, explosion |
| Rules | (377, 138) to (520, 138) and (377, 178) to (520, 178) | One-pixel lines |
| Left-hand block | Lines at y = 144, 154, 164 | See below |
| Right-hand block | Lines at y = 184, 194, 204 | See below |
| Total weight | (407, 224), clip 80 | `inventry.msg` 20, a space, total carried weight |

Each hand block, at x = 377:

- **Empty hand.** Line 1: `inventry.msg` 14, clip 120. Line 2: message 24, a space, then `1-` and the owner's melee damage + 2, clip 120.
- **Item.** Line 1: the item name, clip 140.
  - Armor held in a hand: line 2 is message 18.
  - Weapon: line 2 is message 15, a space and `min-max`, clip 140. Max includes the owner's melee damage when the hand's primary mode is melee or unarmed. When that mode is ranged or its range exceeds 1, the line continues with three spaces, message 16, a space and the range.
  - Weapon with ammunition capacity above 0: line 3 is message 17, a space and `loaded/capacity`, followed by a space and the ammunition name when the weapon has an ammunition PID.
  - Any other item has only line 1.

**Examine text.** An arrow-cursor click, or Look in the action menu, replaces the stats panel. `[Must-match]`

- The panel background is restored.
- The item name goes on line 1 and a rule at y = 64. Then the examine text follows from line 3, then `proto.msg` 540, or 541 when the weight is 1, filled in with the item's weight. A weight of 0 gets no weight line.
- Line n is at y = 44 + 10n, clip 152. Lines past 17 are dropped.
- **Wrapping.** A string wider than 152 pixels is cut at a space: after its first word, or at a later space while the cut text stays narrower than 152 pixels. The rest continues on the next line. A first word wider than 152 drops that line and all remaining text. A string wider than 152 with no space after its first character is drawn clipped at 152.
- Returning to the hand cursor redraws the stats.

**Rotating figure.** `[Must-match]` unless tagged.

- **Art.** The owner's critter art, frame 0:
  - with worn armor, the armor prototype's male or female art index for the owner's gender, or the default player art when that index is -1;
  - otherwise the default player art;
  - the weapon code of the item in the interface bar's active hand when it is a weapon, otherwise 0.
  - A non-critter owner shows its own art.
  - When the player inventory window closes and its owner is the player, the player object takes this art (SPEC-005 §3.8).
- **Draw.** It is centered in the 60 by 100 box, clipped to the box, and drawn transparently. The box is first restored from `loot.frm` (114) at window (176, 37), even in the inventory and use-on windows, whose own art differs there (422 and 1275 differing pixels in the corpus). `[May-improve]`: restore from the window's own background art.
- **Rotation.** The figure turns one facing at a time through facings 0-5. It changes at most once every `trunc(1000 / 6)` = 166 milliseconds, and only on polls of the window loop. The facing counter persists between openings.

**There is no two-click drag.** Every inventory-type window drags with press, hold and release: the player inventory, loot (SPEC-013 §3.16) and barter (SPEC-013 §3.18). The press lifts the item at once, the drag lasts while the left button stays down, and the first poll after release drops it. The mouse layer has no double-click detection, so a second click is just another press. A quick press and release drops the item where it was lifted, which changes nothing. There is no movement or time threshold that separates a click from a drag.

**Cursor modes.** The window opens in the hand cursor (list index 286). Each right-button press switches between hand and arrow (250); returning to the hand redraws the stats panel. While the right button is held, the held-button repeat (section 3.28) switches the cursor again every 250 milliseconds. `[May-improve]`: switch only on the initial right press.

**Lifting** (hand cursor). A left press on a filled slot lifts it. A press on an empty slot does nothing.

| Slot | Rectangle |
| --- | --- |
| List row i, i = 0-5 | (124, 35 + 48i) 64 by 48 |
| Left hand | (234, 286) 90 by 61 |
| Right hand | (325, 286) 90 by 61 |
| Armor | (234, 183) 90 by 61 |

- **Cursor during the drag.** The item's inventory art, frame 0, at its own size (not scaled), with its hotspot at `(trunc(w / 2), trunc(h / 2))`. Drop targets are tested at the pointer.
- **Sounds.** `ipickup1` on lift and `iputdown` on drop, only when the art loads.
- **Source slot.** A single item, a hand item or armor is replaced by the slot background. An item held in both hands clears the whole 180-by-61 hand area. A list stack redraws one smaller.
- **Input.** All other input is ignored during the drag, including scrolling. Only the rotating player figure keeps animating.

**Dropping.** Targets are tested in this order:

1. **List area**, x 124-188 and y 35 through the bottom of the visible rows. The row is `trunc((y - 39) / 48)`, the same 4-pixel offset as SPEC-013 §3.16.
   - A different container item in that row receives the dragged item. A stack opens the move-items counter (SPEC-013 §3.17), and Cancel aborts.
   - A weapon there that accepts the dragged ammunition is reloaded, with the counter for a stack, and plays the weapon's ready sound.
   - Otherwise an item dragged from the list stays put. An item from a hand or armor slot is unequipped into the list, and armor stats are removed. A failed add restores the slot.
2. **Left hand, then right hand.**
   - A different container in the slot receives the item.
   - An empty slot takes the item.
   - Ammunition that fits the weapon in the slot reloads it.
   - Otherwise, when the occupant is a weapon and the dragged item is ammunition that did not fit, nothing happens.
   - When the item came from the other hand, or from the armor slot and the occupant is armor, the two swap.
   - Otherwise the occupant returns to the list and the slot takes the dragged item. From a list stack the slot takes one unit, without the counter. Any item type can be held.
3. **Armor.** Armor only; anything else is ignored. The old armor goes back to the source hand slot when the drag came from a hand, otherwise to the list. Both items' stat effects are updated. If the old armor cannot be stored, the move is reverted.
4. **Player figure**, x 256-316, y 37-137. Only while a nested container is open: the item moves up one container level.
5. **Anywhere else**, including scroll buttons, Done and outside the window: nothing happens.

After every drop the player art, stats and list are redrawn and the hand cursor returns.

**Arrow cursor.** A left press on a filled slot does not drag.

- Releasing before the 250-millisecond hold event shows the item's full examine text in the stats panel. In the loot and barter windows it shows the short look line on the monitor instead.
- Holding opens the action menu at the pointer with the pointer hidden, using the section 3.28 highlight rule. Unlike the world menu, the index is clamped to the list, so reversing direction moves the highlight back at once.
- **Menu art.** The menu is composed as in section 3.28 (`actmenu.frm` 283, 69 by 222, `iaccuxx1` on opening), with 40-by-40 icons: Look `lookn.frm` / `lookh.frm`, Use `usegetn.frm` / `usegeth.frm`, **Drop** `dropn.frm` (255) / `droph.frm` (254), **Unload** `unloadn.frm` (302) / `unloadh.frm` (301), and Cancel `canceln.frm` / `cancelh.frm`. The highlighted art is the list index just before the normal art. The first entry starts highlighted.
- **Menu placement.** The 69-by-222 menu's top-left is at the pointer (after any upward shift), with icons stacked 40 pixels apart starting 29 pixels to its right. When flipped, its top-right is at the pointer.
  - The window's right edge (screen x 579 for the inventory, 372 for use-on, 617 for loot, 560 for barter) replaces the screen edge in the flip test.
  - The window's bottom (377, 376, 376, 470) replaces y 381 in the upward shift of section 3.28.
  - The menu button is clipped to the window's height. On release, the menu area is restored from the window art (for barter, from the barter window) and the pointer returns to where it was when the menu opened.
- Player-window entries:
  - a weapon that can be unloaded: Look, Unload, Drop, Cancel;
  - a container held in a hand or worn: Look, Drop, Cancel;
  - a usable or use-on item: Look, Use, Drop, Cancel;
  - anything else: Look, Drop, Cancel.

**Use-on window.** It has no drag. In the hand cursor, a left press on a list row uses that item on the target at once and closes the window; in combat this needs and spends 2 AP. The list is drawn first-to-last, but the press selects the item counted from the end of the list: row r at scroll offset o picks entry `length - 1 - (o + r)`. The used item can therefore differ from the one shown. `[May-improve]`: use the item drawn in the pressed row.

### 3.35 Working Map Set, Revisits and Aging

Area maps persist through the **working map set**: one `.SAV` file per visited map, in `MAPS\` under the writable data directory. That directory is the `master_patches` path of `fallout.cfg` `[system]`, or `DATA` when the key is absent.

A `.SAV` is the map stream of SPEC-008 §2.9 and SPEC-003, with the saved bit set. It holds:

- every object record: dead critters with their final art, door frames and open-door flags, lock and jam state, and items dropped anywhere;
- the map's global variables (MVARs);
- every script instance with its local variables.

Campaign variables are not in it; they live in the save slot's `SAVE.DAT` (SPEC-008). `[Must-match]`

**Clearing.** `[Must-match]` Every game reset deletes all `MAPS\*.SAV`. A reset happens at program start, on a new game, when a save is loaded (before the slot's copies are restored), and when a session returns to the main menu. A new game therefore always starts from the original `.MAP` files.

**Leaving a map.** `[Must-match]` This happens before another map loads, and when the town map or world map opens (SPEC-013 §3.22):

1. Stop all animations, and prepare the party members and their items for the move.
2. Run the map-exit queue cleanup, then request `map_exit_p_proc` (SPEC-006 §3.5).
3. Re-queue the game-time event, and restore the roof over the player's position.
4. Set the map's saved bit `0x01` and record the current game time as its last visit.
5. If the map number at the player's elevation is in the excluded set, or the map has no number (SPEC-008 §3.10), delete any working `.SAV` for it and write nothing. The excluded set is SPEC-008 §2.8's initial set: maps 0-2, 19-23, 49-54 and 56-65, which are the desert and encounter maps. Those maps are rebuilt from their `.MAP` on every visit. Otherwise write `MAPS\<name>.SAV` and capture the automap for that elevation.
6. Remove every object, the prototype cache and the tile data.

**Loading a map.** `[Must-match]`

- The name comes from `map.msg` by map number and is uppercased. Probe `MAPS\<name>.SAV`.
- If it opens, load the saved copy. Under SPEC-006 §3.5 this means no `.GAM` initialization, and `map_enter_p_proc` receives 0. When that load finishes, including its map scripts, apply the revisit rules below and start the city music.
- Otherwise load `<name>.MAP` fresh and clear the player's last-attacker reference.
- Requesting the current map again still saves it and then reloads it.

**Revisit rules.** These apply only after a saved copy loads, and only when the current game time is not earlier than its recorded last visit. Let h = `trunc((now - last visit) / 36000)`, in whole game hours. `[Must-match]` unless tagged.

1. If h ≥ 24, unjam every lock on the map.
2. If h ≥ 1, process every living critter other than the player and party members: clear its fleeing bit, and unless its kill type is robot, heal it by `14 × trunc(h / 3)` HP when it is below its maximum, without exceeding the maximum.
3. If h > 144 (more than 6 days), replace every dead critter other than the player whose kill type is not robot with remains, in object order:
   - Critter PID `0x01000031` gets item PID 213 (Remains of Gizmo), and its inventory is not dropped; it is destroyed with the corpse. Every other corpse drops its whole inventory on its hex and gets miscellaneous PID `0x05000004` (`rscorpbl.frm`, 9 frames).
   - Place the remains on the corpse's hex and elevation. Draw r = `random(0, 3)` from the shared generator. The frame is r + 6 when bit `0x800` of the critter prototype's behavioral flags (SPEC-003 offset `0x2C`) is set; otherwise r + 3, except r alone for the rat and mantis kill types. A frame past the art's last frame is rejected, and the remains keep frame 0.
   - Cancel the corpse's animations and erase it.

   The draw can reach into the next three-frame group, or past the end of the art. `[May-improve]`: draw `random(0, 2)`, so the frame stays within its group of `rscorpbl.frm` (0-2, 3-5 or 6-8).
4. The reference also has a branch for more than 14 days, which would remove miscellaneous PID `0x0500000B` objects. It is unreachable, because the more-than-6-days test is made first, so such objects are never removed.

**Saving and loading a game.** `[Must-match]`

1. Saving first writes the current map into the working set without leaving it. The map is marked saved with the current time as its last visit, and it is written even when it is an excluded map, because that exclusion is tested only when leaving.
2. Every `MAPS\*.SAV` is then copied into the slot folder, and their names are listed in `SAVE.DAT`. `AUTOMAP.DB` is stored as `AUTOMAP.SAV` (SPEC-008 §2.4-§2.5 and §3.3).
3. Loading erases the working `.SAV` set and `AUTOMAP.DB`, and copies the slot's listed files back.
4. It then loads the current map from its `.SAV` with the revisit rules above. No hours have passed at that point, so they change nothing.
5. Leaving an excluded map that was loaded this way deletes its working `.SAV` again.

Returning to a town therefore shows exactly the state left there, apart from the revisit rules: corpses, opened doors, dropped and moved items, map variables and script state.

### 3.36 Roof Hiding and Restoring

Roofs are hidden by setting the roof-hide bit of whole connected groups of roof squares when the player walks under them, and restored by clearing it when the player walks out. The roof word layout is in SPEC-003 §2.8: roof art index in bits 16-27, roof-hide bit `0x10000000`, flood-barrier bit `0x20000000`. In R1 there is no key, preference or script that turns roofs off as a whole. `[Must-match]`

**Terms.** `[Must-match]`

- The **roof square** of hex h is column `trunc((h mod 200) / 2)`, row `trunc(trunc(h / 200) / 2)`, at grid position `column + 100 × row` of the player's elevation. Each square covers a 2 × 2 block of hexes.
- A square is **empty** when its roof art index is exactly **1**, the blank tile. Any other index counts as a roof, including index 0, which no corpus map uses as a roof.
- The **nibble** of a square is its roof flag bits 28-31: bit 0 is the hide bit, bit 1 the barrier bit.
- **Hide fill** from a square: if the square is inside the 100 × 100 grid, not empty, and has neither the hide bit nor the barrier bit, set its hide bit and continue to its four edge neighbors (left, right, up, down). The result is the edge-connected group of such squares that contains the start. A start square that does not qualify hides nothing.
- **Restore fill** from a square: if the square is inside the grid, not empty, and has its hide bit set, clear the bit and continue to its four edge neighbors. The result is the edge-connected group of hidden squares that contains the start.
- The fills do not connect diagonally. The reference fills recursively; any traversal order gives the same set.

**Tracking state.** The engine keeps the last roof square (column and row), the last elevation, and a last-empty flag. Removing all objects (unloading a map, loading a map, a game reset) sets them to "none" (-1, -1, -1) and last-empty to true. `[Must-match]`

**The rule.** `[Must-match]` Every time the **player** object is placed on a hex — a walking step that crosses into a new hex (SPEC-007 §3.11.1), a placement, a script move, or the entrance placement of a map load — and after that hex's exit grids are checked (SPEC-013 §3.22):

1. Compute the new roof square S on the new elevation. If S and the elevation both equal the last ones, stop.
2. Let E be "S is empty", c the nibble of S, and p the nibble at the last square's position, read from the **new** elevation's grid.
3. If E differs from last-empty, or c differs from p:
   - (3a) If last-empty is false, run the restore fill from the last square, on the new elevation.
   - (3b) If E is false, run the hide fill from S.
4. Record S, the elevation and E as the last ones, whether or not step 3 ran.

Only the player triggers this. Party members and other critters never hide or restore roofs.

**Consequences.** `[Must-match]`

- Stepping under a roof hides its whole edge-connected group at once, so roofs of buildings that touch along an edge vanish together. Stepping within that group changes nothing, because every square of it has nibble 1.
- Stepping out onto an empty square restores the group.
- A hex step can move to a diagonally adjacent square. Two roof groups that touch only at a corner are separate groups: stepping from one to the other compares nibble 1 with nibble 0, so it restores the first and hides the second.
- A square with the barrier bit is never hidden and stops both fills. Standing on it hides nothing, so its roof stays drawn over the player. No corpus map sets the barrier bit.
- The hide bit also changes pointer picking: a hidden roof square no longer blocks objects under the pointer (section 3.27).

**Loading and leaving a map.** `[Must-match]`

- Loading a map clears the hide bit on every square of each tile block present in the file (SPEC-003 §2.8). Hidden roofs saved in a `.SAV` therefore come back drawn. The entrance placement, a placement from "none", then hides the group under the player, if any.
- Leaving a map (section 3.35 step 3): if the last square, read on the player's current elevation, is not empty, run the restore fill from it before the map is written.

**Defects.**

- The first placement after a reset reads the "previous" nibble at position `-1 + 100 × -1`, 101 words before the start of the grid. With last-empty still true, the value read never changes the result. `[May-improve]`: when there is no last square, skip the nibble comparison and test only E against last-empty.
- On an elevation change, step 2 reads p and step 3a restores on the **new** elevation, at the old square's position. A group hidden on the old elevation therefore stays hidden there. It reappears only when the player later enters and leaves it on that elevation, or when the map is reloaded. A matching group on the new elevation at the old position, if hidden, is restored instead. `[May-improve]`: run the restore fill on the elevation where the group was hidden, and take p from that elevation.

## 4. Synthetic Test Fixtures (Hex / Text)

These vectors are invented and computed from the rules above; none is a shipped asset.

### 4.1 Fixture A: Splash Header

The first 16 bytes of a synthetic 307978-byte splash:

```text
0000: 52 49 58 33 80 02 E0 01 AF 00 3F 00 00 00 3F 00
```

Bytes `0x00`-`0x09` are skipped. Palette entry 0 is the levels (63, 0, 0) at `0x0A`; entry 1 begins (0, 63, ...) at `0x0D`. Pixel (0, 0) is the byte at `0x030A` and pixel (639, 479) the byte at `0x4B309`. Width `80 02` and height `E0 01` read as little-endian 640 and 480, but the reference does not read them. With `splash` = 9 and only `splash0.rix` present, the names tried are `splash9.rix`, then `splash0.rix`; `splash0.rix` is shown and the stored value becomes 1.

### 4.2 Fixture B: Layout Arithmetic

| Input | Expected |
| --- | --- |
| Main-menu button 3 | Rectangle (425, 168) 26 by 26; label top y = 169 |
| Main-menu label 60 pixels wide | Left x = 490 |
| Save/load click at y = 112 / 113 / 121 | Slot 0 / 1 / 1 (reference); slot 0 / 0 / 1 under the safe rule |
| Thumbnail destination column 0, 1, 222 | Source columns 2, 5, 637; column 223 unwritten |
| Thumbnail destination row 131 | Source row 377 |
| Game tick 264600 | Info line time `0721` |
| Maximum HP 50: thresholds `trunc(50 * 0.25)` = 12 and `trunc(50 * 0.5)` = 25 | HP 11 red; HP 12 and 24 yellow; HP 25 white |
| AP 13 in combat on the player's turn | 10 green pips at x = 316, 325, ..., 397 |
| Text delay 3.5 | Slider knob x = 493 |
| Master volume 22281 | Slider knob x = `trunc(22281 * 219 / 32767 + 384)` = 532 |
| Brightness after 16 increments from 1.0 | 1.17999267578125, the upper bound; a 17th increment does nothing |
| Indicator boxes CHEM, SNEAK, LEVEL | Box x = 0 (127 wide), 124, 251; window width 381 |
| Arrow pick art at pointer x = 572 | `572 + 68 = 640`: mirrored `actarrom.frm` |
| Action menu with 5 entries at pointer y = 300 | `300 + 200 - 1 = 499` is at least 381: shift up by 120 |

### 4.3 Fixture C: Editor Rules

Start from a blank sheet: SPECIAL all 5, 5 character points, three tag picks, no traits.

| Action | Expected |
| --- | --- |
| Press Strength plus five times | ST 10, points 0 |
| Press Strength plus once more | No change; repeat stops |
| Press Done | `iisxxxx1`, `editor.msg` 142 / 143 (tag picks remain); points are 0, so 118 / 119 is not shown |
| Tag Small Guns, Lockpick, Sneak; press Done | Accepted; name "None" accepted |
| With Bruiser picked (+2 Strength), base ST 8 and 2 points left | Shown ST 10; plus is refused because the shown value is not below 10 |
| Shown value 11 from bonuses at Done | 157 / 158 |
| Age dialog at 35, press `+` | Stays 35, redraw without animation |

### 4.4 Fixture D: Endgame Slide Selection

| Globals (all others 0) | Slides shown, in order |
| --- | --- |
| None set | `dp` (`nar_10`), `seq4abc` (`nar_19`), `seq5b` (`nar_25`), `seq6a` (`nar_28`), `seq8abc` (`nar_37`), `dp` (`nar_40`) |
| 51 = 1, 30 = 1, 31 = 2, 132 = 1, 604 = 1, 26 = 1, 38 = 1, 108 = 2, 250 = 1, 606 = 1, 69 = 2, 115 = 6 | `dp` (`nar_11`), `seq2b` (`nar_13`), `seq3a` (`nar_16`), `seq4de` (`nar_22`), no step-5 slide, `seq6b` (`nar_29`), `seq7a` (`nar_32`), `seq8abc` (`nar_36`), `dp` (`nar_40`) |
| 104 = 2, 69 = 2, 114 = 1, 115 = 7 | Step 5 `seq5a` (`nar_24`); step 8 `nar_35` |

In the first row step 4 shows `nar_19` because global 604 is 0 and global 26 is 0. Global 299 is 0 afterward in every row.

### 4.5 Fixture E: Credits Text

```text
;comment line
@PRODUCER
Jane Example
#Grey Name

@DESIGN
```

Expected in normal style: rows 0-4 are title `PRODUCER` (font 103), name `Jane Example` (font 104), grey name `Grey Name` (font 104, key 17969), an empty row, title `DESIGN`. The comment produces no row. Each row is 26 pixels, so the last row enters after 5 * 26 = 130 ticks, then 480 blank ticks follow. Typing `b`, `o`, `x` ends the credits at `x`; typing `b`, `o`, `o`, `m` with violence level above 0 starts the `tblcrd.frm` animation instead.

### 4.6 Fixture F: Monitor Wrapping

With a bullet width of 6 pixels, the first line of a message holds text narrower than 155 pixels; continuation lines hold text narrower than 161. A 154-pixel first chunk fits; a 155-pixel chunk breaks at its last space. Seven consecutive one-line messages leave the oldest of them out of view; the six visible lines are drawn at x = 23 through 28, top to bottom.

### 4.7 Fixture G: Roof Hiding

A synthetic elevation-0 roof grid: squares with columns 10-12 and rows 20-21 form one group with roof art index 5; square (13, 22) has index 5 and touches that group only at a corner; every other square has index 1. All nibbles start at 0. The player starts after a map load with the tracking state at "none" and is placed on these hexes in turn:

| Step | Hex | Square | E | c / p | Action | Hidden squares afterwards |
| ---: | ---: | --- | --- | --- | --- | --- |
| 1 | 8018 | (9, 20) | empty | 0 / read before the grid | Emptiness unchanged (true); nothing to restore or hide | None |
| 2 | 8020 | (10, 20) | roof | 0 / 0 | E changed: hide fill from (10, 20) | The 6 squares (10-12, 20-21), roof words now `0x1005xxxx` |
| 3 | 8422 | (11, 21) | roof | 1 / 1 | No change | Same 6 |
| 4 | 8424 | (12, 21) | roof | 1 / 1 | No change | Same 6 |
| 5 | 8826 | (13, 22) | roof | 0 / 1 | Nibble changed: restore from (12, 21), hide from (13, 22) | (13, 22) only |
| 6 | 8828 | (14, 22) | empty | 0 / 1 | E changed: restore from (13, 22) | None |

Here `xxxx` is the unchanged floor half. If square (13, 21) also had index 5, it would join both groups along edges, step 2 would hide 8 squares and step 5 would change nothing.

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings

Verification notes: `tools/spec_lint.py` gives 0 errors, and no source path or raw revision hash appears. Twelve public `fallout.cfg` key literals that coincide with reference identifiers were added to the linter allowlist (CLAUDE.md rule 2): `game_difficulty`, `violence_level`, `target_highlight`, `combat_messages`, `item_highlight`, `combat_speed`, `player_speedup`, `mouse_sensitivity`, `master_volume`, `music_volume`, `sndfx_volume` and `speech_volume`. The 33 plain-word warnings were reviewed and are benign: the configuration keys "splash", "brightness", "running" and "subtitles", and the configuration section name "system"; the sound name "monitor"; and formula words ("line", "time", "length", "width", "text", "current", "count", "trunc", and "column" in the section 3.36 roof-square formula). Campaign variables are cited by index only. The only fenced blocks are a synthetic hex dump and a credits text sample. The sound name "toggle" is also flagged. The section 3.29 border values were recomputed with an independent model of the hex-to-screen transform and checked against the entry hexes of all 72 corpus maps. Section 3.34 art sizes were read from the corpus FRM headers. Section 3.35 file and frame facts were checked against the corpus (`rscorpbl.frm` has 9 frames; item PID 213 is Remains of Gizmo). The section 3.27 busy-click rules were checked against SPEC-007 §3.10.1 ("One sequence per owner"). The section 3.36 empty-roof test and bits match SPEC-003 §2.8, and the roof words of all 72 corpus maps were scanned: 15056 roofed squares, none with roof index 0 or the barrier bit; `LAGUNRUN.MAP` stores 257 squares with the hide bit set, which loading clears. Fixture G was stepped by hand through the section 3.36 rule. Tag count, excluding the two tag definitions: 148 `[Must-match]`, 57 `[May-improve]`.
