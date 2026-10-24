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
- the character screen in game (folders, skill spending, perk chooser, Tag! and Mutate!) and the character-sheet file dialogs and printed sheet;
- the save/load slot screens, quick save and quick load;
- the in-game options menu, pause window, quit confirmation, preferences screen and generic dialog box;
- the bottom interface bar: buttons, message monitor, HP/AC counters, AP pips, active item slot, indicator boxes and combat panel animation;
- mouse cursor shapes and modes: movement hex, action pick, action menu, targeting crosshair, edge scrolling and busy cursors;
- the in-game automap window and the Pip-Boy automap page;
- the Pip-Boy screen: opening, art and layout, date and note, Status and holodisk pages, Automaps, Video Archives, the Alarm Clock and its rest display, and the screen saver (section 3.41);
- the world map screen and the town map screen: art, view and scrolling, fog drawing, markers, encounter flashes, the side panel, entrance markers, input, and what each exit leads to.
- the player inventory and use-on windows: art, layout, stats panel, rotating figure, action menu, cursor modes and item dragging;
- the endgame slideshow, ending movie and scrolling credits;
- the working map set: how visited maps persist across map changes, saves and loads, and how a revisited map ages;
- when roofs are hidden as the player walks under them and restored when the player walks out;
- the draw order of map objects, their lighting, and the translucent draw modes (section 3.44).

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
| Pip-Boy screen | Pip-Boy module, rest eligibility test, word wrapper, game-mouse enable and disable, button and window library |
| World map and town map | World-map module, window button module, script request dispatcher, indicator-box and color layers |
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
- **R1-rev7 (2026-10-21).** New section 3.37 Player See-Through Cutout: the `egg.frm` mask, its position on the player, roofs, the wall and scenery orientation table, compositing, and two `[May-improve]` failure cases. Section 3.27's pointer rule now uses it. Section 3.33 gives the Pip-Boy automap cell as 2 by 1 pixels on a two-row pitch, matching SPEC-008 §2.8. Section 3.34's drop-row formula is now `[May-improve]`, as in SPEC-013 §3.16.
- **R1-rev8 (2026-10-22).** Vanilla-parity pass: the remaining interface stand-ins and Builder questions.
  - New section 3.38 Called-Shot Window: the attack preflight and its `combat.msg` refusals, `called.frm` (there is no `targbox.frm`), the `na` critter picture, the eight body-part rows and chance digits, labels chosen by art alias for every corpus alias, hover, Cancel, the Ctrl+A-G input codes (the digit keys do nothing), and two out-of-table `[May-improve]` defects. Section 3.23 gains the slot attack query: reload, use and use-on report no attack. New Fixture H.
  - New sections 3.39 World Map Screen and 3.40 Town Map Screen: `worldmap.frm` (there are no `wrldmp` tiles), view, scrolling, fog drawing, markers, travel dots, encounter flashes, the side panel and its town buttons, the town pictures `twnmap00`-`twnmap11`, the full entrance table (Vault 15 is `VAULTENT.MAP` and `VAULTBUR.MAP`), and the rule that arriving at a town never enters it. New Fixture I.
  - New section 3.41 Pip-Boy Screen: `pip.frm` and its companion art (there is no `pipboy.frm`), opening and closing, buttons and keys, date, time and the vault-water note, the text area and row zones, the Status, holodisk, Automaps, Video Archives and Alarm Clock pages, the screen saver, and every `pipboy.msg` ID. New Fixture J.
  - New section 3.42 Character Screen Outside Creation Mode (`edtredt.frm`, level block, skill spending, the Perks, Karma and Kills folders, condition lines, the info-card mapping, the perk chooser, Tag! and Mutate!) and section 3.43 Character Sheet Save, Load and Print (file list, load and save dialogs, messages, the printed sheet). Section 2.4 lists the added message IDs. New Fixtures K-P.
  - Section 3.27: the combat input lock is a combat-wide count of running sequences; no key acts during the lock, and held keys are flushed at each player turn; the animated watch shows on every NPC turn; talk re-tests distance and line after its approach. Section 3.23: every key and click read during a hand swap is discarded except F12.
  - Sections 3.35 and 3.36: the working `.SAV` name follows SPEC-008 §3.10, and the roof restore on leaving a map is unaffected by the elevation safe rule.
- **R1-rev9 (2026-10-22).** New section 3.44 Map Object Draw Order, Lighting and Translucency: per-hex list order (flat first, then insertion order; every offset step reinserts), the fixed visiting lattice in ascending hex number, the flat and upright passes, the 100-outline limit, and the five translucent draws with their colors and gray tables. Section 3.37 points to it. New Fixture Q.
- **R1-rev10 (2026-10-23).** The last open Builder questions.
  - New section 2.5 Screenshot File, and the F12 rules in section 3.19: `scrNNNNN.bmp` in the working directory, an 8-bit BMP, and `misc.msg` 3 and 8.
  - Section 3.19 gives the `/` and `?` date line format.
  - Section 3.13 gives the medium Done box offsets; a medium Yes/No box cannot occur.
  - Section 3.20: the message monitor's scroll areas are disabled with the bar.
  - Section 3.29: every key auto-repeats, window toggles included.
  - Section 3.17 gives the pause grey table and the converted rows.
  - Section 3.44 gives the outline word, the outline ring geometry and colors, what reinserts an object in its hex list, and the floor light map.
  - Section 2.3 gains the `music` key.
  - New section 3.45 Dialogue Review and Ask About Windows.
  - New Fixtures R, S and T.
- **R1-rev11 (2026-10-23).** Engine code-review V2 questions.
  - Section 3.21: clicks released during a counter roll or a combat-panel animation are lost; keys wait in the keyboard buffer and follow the section 3.27 flush rules. Section 3.25 states the same for the panel.
  - Section 3.27: the movement-click order (cancel, walk or run and sneak, registration, route) and the no-route stop. The player stops silently and the old path is not resumed.
  - New section 3.46 Floating Text and Fixture U.
  - Section 3.41.1 points to the SPEC-012 §3.12 water countdown.
- **R1-rev12 (2026-10-23).** The last README questions.
  - Section 3.29: modifier keys repeat but translate to no key code (item 45).
  - Section 3.27: the in-world cursor is a flat map object drawn in the section 3.44 flat pass.
  - Section 3.44: no R1 sequence uses the outline animation step (item 47).
  - Section 3.18: the Combat messages row points to SPEC-007 §3.12.

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
| sound | music | 0-1 | 1 | SPEC-008 §3.10 (music on/off state) |

### 2.4 Message Resources

All files are under `text/<language>/game/`. The listed indices are exact data; texts are the English corpus strings. `[Must-match]`

| File | Indices used |
| --- | --- |
| `misc.msg` | 0 "Are you sure you want to quit?"; 3 "Saved screenshot."; 4 "Quick load game successfully loaded."; 5 "Quick save game successfully saved."; 7 "Pipboy not available in combat!"; 8 "Error saving screenshot."; 9-13 menu labels INTRO, NEW GAME, LOAD GAME, CREDITS, EXIT; 14 copyright line; 15 "Action Points"; 16 "Hit Points"; 6000 "Ask about what?" (section 3.45) |
| `lsgame.msg` | 100-103 screen titles; 104 DONE; 105 CANCEL; 106-108 slot-list error; 109 SAVE GAME; 110 LOAD GAME; 111 "- EMPTY -"; 112 "- CORRUPT SAVE  FILE -"; 113 "- OLD VERSION -"; 114 "Empty."; 115 "Error!"; 116 "Old version."; 117-128 JAN-DEC; 130 DESCRIPTION; 131 overwrite question; 132-133 save error; 134-136 load error; 140 "Game Saved."; 141 "Game Loaded." |
| `options.msg` | 0-4 options buttons; 100 GAME PREFERENCES; 101-122 preference captions and buttons; 201-224 value labels; 300 DONE; 301 PAUSED |
| `dbox.msg` | 100 DONE, 101 YES, 102 NO, 103 CANCEL (file dialogs, section 3.43; Ask About, section 3.45) |
| `editor.msg` | 100-102 DONE / OPTIONS / CANCEL labels; 103 PRINT; 104 AGE; 107 MALE; 108 FEMALE; 109-115 in-game folder tabs, SKILL POINTS and level block; 116-158 editor captions, cards, refusals and validation; 200-209 stat level words; 300-318 condition and derived labels; 400-406 condition descriptions; 500-511 month names; 600-618 editor options and file messages; 620-655 printed sheet; 1000-1009 and 1100-1109 karma names and descriptions. Per-ID use is tabled in section 3.42 |
| `proto.msg` | 450-464 kill-type names; 465-479 kill-type descriptions (section 3.42); 650 "[Done]"; 903 "This person will not barter with you."; 904 "This person has nothing to tell you about." (section 3.45) |
| `perk.msg` | 101 + perk names; 201 + perk descriptions (SPEC-011 §3.22) |
| `intrface.msg` | 100 SNEAK, 101 LEVEL, 102 CHEM |
| `map.msg` | Location short names and elevation names (section 3.14) |
| `stat.msg`, `skill.msg`, `trait.msg` | Names and descriptions for the selection screen and editor (SPEC-006); `stat.msg` 301-310 in-game level words and 401-402, 501-502 level and experience cards; `skill.msg` 300-317 skill-card formulas (section 3.42) |
| `pipboy.msg` | 100-107 holidays; 200-203, 205, 206, 210-212, 214, 215 controls and titles; 300-314 alarm clock; 400-417 holodisk titles and their text; 503-513 archive titles; 700-810 quests (section 3.41.9) |

### 2.5 Screenshot File

F12 writes the composed screen to a Windows bitmap file. All multi-byte fields are little-endian. For the 640-by-480 screen the file is 308278 bytes. `[Must-match]`

| Offset | Length | Value |
| --- | ---: | --- |
| `0x0000` | 2 | `42 4D`, ASCII `BM` |
| `0x0002` | 4 | File size, `width × height + 1078` |
| `0x0006` | 4 | 0 (two reserved 16-bit words) |
| `0x000A` | 4 | 1078, the pixel data offset |
| `0x000E` | 4 | 40, the information header size |
| `0x0012` | 4 | Width, 640 |
| `0x0016` | 4 | Height, 480, positive, so rows are stored bottom-up |
| `0x001A` | 2 | 1 plane |
| `0x001C` | 2 | 8 bits per pixel |
| `0x001E` | 24 | Six 32-bit zeros: compression, image size, horizontal and vertical resolution, colors used, important colors |
| `0x0036` | 1024 | 256 palette entries of 4 bytes: blue, green, red, then 0. Each level is the palette's 6-bit level multiplied by 4 (0-252) |
| `0x0436` | `width × height` | Palette indexes, bottom screen row first. Rows have no padding |

The palette is the one the game last installed (section 3.2), before the brightness table of SPEC-001 §3.1 is applied, so the file ignores the brightness setting. `[Must-match]`

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
- **Print to file** (`*.TXT`, default name 616) inverts its existence test: it asks "already exists / overwrite?" when the file does not exist and overwrites an existing file silently. `[May-improve]`: prompt only when the file exists. The file dialogs, every message of the three operations and the printed sheet layout are in section 3.43.
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

- **Done box.** `donebox.frm` (108 by 24) is copied opaquely, centered horizontally: x = `trunc((box width - 108) / 2)`. Its top is at y = 98 in the large box and y = 81 in the medium box. The caption, `dbox.msg` 100 in font 103 with key 18979, goes at (x + 35, top + 3). The red button goes at (x + 13, top + 4).

  | Box | Done box | Caption | Red button |
  | --- | --- | --- | --- |
  | Large | (97, 98) | (132, 101) | (110, 102) |
  | Medium | (53, 81) | (88, 84) | (66, 85) |

  A medium box always has this single Done button. Yes/No and two-button boxes are forced large, so no medium Yes/No layout exists. `[Must-match]`
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

**Pause window.** Background `charwin.frm` (139 by 72) with `donebox.frm` drawn transparently at (13, 42). The window is at screen (250, 150) in game, or (185, 180) on the world map. It shows "DONE" (`options.msg` 300) at (52, 45) in font 103, key 18979, and "PAUSED" (301) in font 104, key 18979, at y = 10, x = `2 + trunc((139 - w) / 2)`. A red button sits at (26, 46). Before the window opens, the view is converted to greyscale through the grey lookup table below. In game the pointer is hidden, the whole map is redrawn, and every pixel of the 640-by-381 map window is converted, rows 0-380. Rows 379 and 380 lie under the interface bar, which is not converted and stays on top, so screen rows 0-378 turn grey. On the world map only the region (22, 21) 450 by 442 of the world-map window is converted. In game, background processing and palette cycling are suspended.

**Grey lookup table.** The table maps each palette index i to a grey index. `[Must-match]`

- Take P(i), the three 5-bit components of entry i of the `color.pal` palette as loaded (each 6-bit level halved and truncated). Unmapped entries count as zero (SPEC-001 §3.7). In the corpus these are entries 0 and 229-255.
- Let s be the largest component plus the smallest, from 0 to 62.
- Let v = `trunc(s × 240 / 510)`, from 0 to 29.
- The grey index is the `color.pal` RGB555 lookup of the color (v, v, v), that is, of key `1057 v`.

The table is built at program start and again at every game reset, from the `color.pal` palette loaded at that time. The brightness setting and palette cycling have no effect on it. Unmapped entries, including the cycling colors 229-255, therefore become key 0 [228], black. In the corpus white, index 220, becomes index 1 (v = 29), and index 1 becomes index 2 (v = 27), so the grey image is slightly darker than the original. The converted pixels are not restored when the window closes. In game the map is redrawn then. On the world map the region stays grey until the world-map screen next redraws its view (section 3.39), which happens on the next loop pass while follow mode is on. `[Must-match]` The window closes on any key or button event except the brightness keys, which adjust brightness and keep it open; mouse movement does not close it. In game the map is redrawn on close. `[Must-match]`

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
| Combat messages | Switch at (299, 74) | Left 211 "Verbose" = value 1; right 212 "Brief" = value 0. The setting changes only the struck-target line of a critical hit (SPEC-007 §3.12) |
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
| `/`, `?` | Print the date and time to the monitor, also while the bar is disabled. The line is the month name (`editor.msg` 500 + month - 1), a colon and a space, the day, `/`, the year, a space, then the hour without a leading zero, `:` and two-digit minutes. Example: "December: 5/2161 7:21" |
| Ctrl+Q, Ctrl+X, F10 | Quit confirmation |
| Ctrl+P | Pause |

In the combat input loop, Space ends the turn, Enter requests the end of combat, and the loop also ends when the player's AP reaches 0. `[Must-match]`

**Screenshot, F12.** `[Must-match]`

- F12 is caught when the key event enters the common input queue, before any screen sees it. It therefore works in every screen and loop that reads input through that queue, and no screen's own key handling ever receives it. It does not work while the keyboard is disabled (section 3.27).
- The whole screen is recomposed from all windows into a 640-by-480 buffer, including the mouse cursor when it is visible. The display itself is not updated by this pass.
- The game then tries the names `scr00000.bmp`, `scr00001.bmp` and so on, five zero-padded digits, up to `scr99999.bmp`. It looks in the process's current working directory, not the save folder, and takes the first name that cannot be opened for reading. A gap in the numbering is therefore filled first.
- The file is written as section 2.5 describes, and `misc.msg` 3 "Saved screenshot." is printed to the monitor.
- If all 100000 names exist, or the file cannot be created, `misc.msg` 8 "Error saving screenshot." is printed instead. Write errors after the file is opened are not detected, and success is still reported.
- No sound plays.

`[May-improve]`: report a failed write as an error.

### 3.20 Message Monitor

`[Must-match]` unless tagged.

- Area (23, 24) 167 by 60 in the bar; its background is saved once from `iface.frm` and restored before each redraw. Font 101 (line height 10), so **6 lines** are visible; all text key 992 [215].
- Storage is a ring of 100 lines of up to 79 characters. Each new message begins with the bullet character `0x95` (5 pixels plus 1 spacing). Wrapping: a chunk fits while its width is **less than** `167 - 6 - bullet width`, which is 155 pixels for the first line and 161 for continuation lines without a bullet; breaks occur at the last space. A word too long for a line is stored truncated and clipped at 167 pixels.
- Visible lines are drawn oldest at top, line k (k = 0-5) at x = 23 + k: each line one pixel further right.
- A new message snaps the view to the newest line. Out of combat it plays `monitor` at most once per 500 milliseconds.
- Two invisible buttons scroll: the upper half (23, 24) 167 by 30 scrolls one line older with cursor `suparrow.frm`; the lower half (23, 54) 167 by 30 scrolls one line newer with `sdnarrow.frm`; leaving either restores the arrow. Scrolling newer stops at the newest line. Scrolling older stops at the oldest ring slot, so early in a game the view can scroll into empty lines. `[May-improve]`: stop at the oldest nonempty line.
- Each button scrolls once on the mouse press and makes no sound; holding it does not repeat. Both buttons belong to the interface bar's enable state. When the bar is disabled, for example during the combat input lock (section 3.27), they are disabled too. While disabled, a press does not scroll and the pointer entering or leaving them does not change the cursor. They return when the bar is enabled again. `[Must-match]`

### 3.21 HP and AC Counters

`numbers.frm` (360 by 17) holds three 120-pixel color bands: white at x = 0, yellow at 120, red at 240. Within a band, digits 0-9 are 9 pixels wide at x = 9d. The "roll down" glyph is at 90, "roll up" at 99, minus (6 pixels wide) at 108 and the plus/blank sign (6 pixels) at 114. A counter at (x, y) is 33 by 17: sign at x, hundreds at x + 6, tens at x + 15, ones at x + 24. Values clamp to -999 through 999. HP is at (473, 40); AC is at (473, 75), always white. `[Must-match]`

HP band: red when HP is below `trunc(max HP * 0.25)`, yellow when below `trunc(max HP * 0.5)`, otherwise white. `[Must-match]`

**Unanimated update.** Draw the new value's sign and digits at once. A digit cell shows the magnitude digit, `trunc(|v| / 100)`, `trunc(|v| / 10) mod 10` and `|v| mod 10`; the sign cell shows plus/blank for v at least 0 and minus otherwise. `[Must-match]`

**Animated roll.** The counter moves one unit at a time, like a mechanical drum, from the previously shown value P to the new value V (V clamped first). `[Must-match]`

- **Step delay.** `t = trunc(250 / (|P - V| + 1))` milliseconds, computed once from the whole change. Each "wait" below is t milliseconds. The game is blocked until the roll ends. Clicks made during the roll are lost and keys are deferred ("Input during rolls and panel animations", below).
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

**Input during rolls and panel animations.** `[Must-match]` An animated HP or AC roll and the animated open or close of the combat panel (section 3.25) block the game until they end. Neither the input queue nor the bar buttons are read during them, so no click, key or button acts. Mouse buttons and keys are then treated differently.

- **Mouse buttons are sampled, not queued.** The game reads the mouse as a current position and a current button state. It keeps no record of earlier presses and releases.
  - During a roll the state is read once before each wait. The exception is the wait after the tens-cell "roll down" glyph, where nothing is read and the pointer does not move.
  - During a panel animation the state is read continuously while the animated watch is the cursor. In R1 the watch is showing in every path that animates the panel.
  - Each reading replaces the previous pressed, held and released flags, and nothing acts on them.
- **Lost clicks.** A click whose release is read during a roll or panel animation is lost. No map action, bar button or action-menu hold results. A press and release that both fall between two readings are never seen at all. Nothing is queued for later.
- **Held buttons.** A button still held when the animation ends counts as held from the reading that first saw it down. Its release is read after the animation as an ordinary release (section 3.27). It acts at the release position if the game mouse is enabled at that moment. After the turn-start AC roll the lock lifts as soon as the player's turn begins, so such a release acts. During the lock between player turns it is discarded (section 3.27).
- **Pointer and cursor.** The pointer keeps moving, and the animated watch keeps animating. Out of combat the in-world cursor and its hover keep updating. Edge scrolling works during a roll whenever it is enabled, because the combat lock leaves it on (section 3.27). It is off during both panel animations.
- **Keys are deferred.** Key presses and releases stay in the 32-event keyboard buffer of section 3.27. They are read, in order, the next time the game reads input. Whether they then act follows the flush rules of section 3.27:
  - The AC roll at the start of the player's turn comes after that turn's flush. Keys typed during it are read once the turn begins.
  - The AC roll at the end of the player's turn and the combat-start panel animation fall inside the lock between player turns. Keys typed then are flushed at the start of the next player turn, unless combat ends first.
  - Keys typed during the combat-end panel animation, or during the AC roll that follows it, are read after combat ends.
  - Keys typed during an HP roll out of combat, such as healing in the inventory, are read when the roll ends.
- A roll whose value does not change has no waits and blocks nothing.

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
4. While the animation plays, the game waits in a modal loop. The bar's buttons and the game mouse are disabled, and the cursor is `wait2.frm`. Out of combat edge scrolling is also off. In combat on the player's turn it stays on, because the combat input lock (section 3.27) already disabled the game mouse with scrolling left on. Every input event read during the wait, key or click, is consumed and discarded: no hotkey acts, including Ctrl+Q, Ctrl+X, F10, Ctrl+P, F6, F7, the arrow keys, Space and Enter. Only F12 still takes its screenshot, because that key is handled as soon as it is read. In combat on the player's turn the keyboard is not read at all, so keys typed during the swap are held and read once the lock ends (section 3.27). The wait ends early when a quit request is pending. Afterwards the cursor returns to the in-world cursor, and the bar is re-enabled only if it was enabled before the swap. `[Must-match]` If the swap's sequence cannot be registered, there is no wait and nothing is disabled, and the active hand is flipped without the slot being redrawn. `[May-improve]`: redraw the slot when the sequence fails to register.
5. Afterwards a crosshair or use mode returns to movement.

When combat ends the hands are refreshed with animation if their items changed. `[Must-match]`

**Attack query.** Other modules ask the active slot for its current attack, which is a hit mode and an aimed flag. `[Must-match]` unless tagged.

- Primary and primary aimed report the hand's primary hit mode (section 2.4 of SPEC-007: 0 or 2 for a weapon, 4 for an empty hand).
- Secondary and secondary aimed report the secondary hit mode.
- The aimed modes report the aimed flag as true.
- Reload, use and use-on report **no attack** with the aimed flag false. No hit mode is reported. The reload hit modes 6 and 7 are used only by the slot's own reload cost and AP check.

Reload mode and crosshair mode can coexist. This happens when the slot is cycled to reload after a crosshair was set from it, or when crosshair is chosen by right-click or `m` in combat with a weapon in reload mode. While the slot reports no attack:

- Crosshair hover shows the red ` X ` over every critter (section 3.27).
- A crosshair click does nothing. No attack is made, no message prints, no AP are spent, and out of combat no combat starts (section 3.38).
- A player attack made at torso or uncalled by another path reads the aimed flag as false for its AP debit (SPEC-007 §3.4).
- A Ctrl+movement click in combat (section 3.27) subtracts the cost of a hit mode that was never reported, so the amount is undefined. `[May-improve]`: subtract 2, the cost the slot shows for reload, use and use-on.

### 3.24 Indicator Boxes

The indicator boxes occupy a separate window at screen (0, 358), height 21, which exists only while the bar is visible and boxes are enabled. Each box is `warnbox.frm` (130 by 21). Labels come from `intrface.msg` 100 SNEAK, 101 LEVEL and 102 CHEM, in font 101: green key 992 for SNEAK and LEVEL, red key 31744 for CHEM. A label is centered horizontally and placed at y = `trunc((24 - 10) / 2)` = 7, using a box height of 24 rather than 21. Boxes always appear in the order CHEM, SNEAK, LEVEL, with at most four slots. The first box has its left 3 pixels cut and is drawn 127 wide at x = 0; box k for k of 1 or more is drawn at x = `130k - 3(k + 1)`. The window width is 127 times the box count. `[Must-match]`

### 3.25 Combat Panel Animation

`endanim.frm` (57 by 58, 5 frames at 5 fps, 200 milliseconds each) is blitted opaque at bar position (580, 38); the redraw rectangle is (580, 38) to (637, 96). `[Must-match]`

- **Open** (entering combat): play `iciboxx1`; show frames 0 through 4, the first at once and then one every 200 milliseconds, busy-waiting with the mouse active; create the End turn and End combat buttons; then disable them (red light). The non-animated open, used when loading, draws only frame 4.
- **Close** (normal combat end, with map scrolling briefly disabled): delete the buttons; play `icibcxx1`; show frames 4 through 0 at 200 milliseconds. The non-animated close draws frame 0, and loading closes without animation.
- **Lights.** The start of the player's turn enables the buttons, draws `endltgrn.frm` (57 by 58) transparently over the panel and plays `icombat2`. The end of the player's turn, or a just-opened panel, disables them, draws `endltred.frm` and plays `icombat1`.

- **Input.** The animated open and close take 800 milliseconds: the first frame is shown at once and the other four follow 200 milliseconds apart. Clicks released during them are lost and keys are deferred, as section 3.21 states ("Input during rolls and panel animations"). `[Must-match]`

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

**Cursor objects in the draw order.** `[Must-match]` The in-world cursor is an ordinary map object on the current elevation, marked flat (object flag `0x08`), no-block, shoot-through, light-through and temporary. Every mode's art is shown on this one object. It is therefore drawn in the **flat pass** of section 3.44, not as an overlay:

- It is drawn after the flat objects of lower-numbered hexes and before those of higher-numbered ones. Within its own hex it follows the order of insertion, and every move reinserts it at the end of that hex's flat group.
- Every upright object, roof and floating text is drawn over it.
- In movement mode its translucent outline is added to the outline list when it is drawn, so the outline is drawn after the roofs, like every outline.

The AP-text object of movement mode is a separate object that is not flat, so it is drawn in the upright pass at its hex.

**Mode cycling.** A right-click in the map area (y at most 379, ignoring held repeats) or `m` selects mode `(current + 1) mod 3` over movement, arrow and crosshair. Out of combat a resulting crosshair becomes movement. In combat crosshair is skipped only when the active hand holds a non-weapon; an empty hand counts as a weapon. Entering crosshair turns hostile outlines on, and leaving it turns them off. `[Must-match]` The same formula applies to use and skill modes by their numeric values: use crosshair goes to arrow; First Aid, Steal and Repair go to crosshair (or movement); Doctor and Traps go to movement; Lockpick and Science go to arrow. `[May-improve]`: a right-click from use or skill mode goes to movement.

**Hover.** Feedback is computed once the pointer has rested on one pixel for at least 250 milliseconds, and again only after it moves; moving resets the art and removes any item outline. `[Must-match]`

- **Rest timer.** It restarts on any one-pixel move of the hotspot, on a mode change, and after the action menu closes. When the in-world cursor is turned back on, the timer is back-dated by 250 milliseconds, so hover is computed on the next idle poll. This happens on entering the map window, after an edge scroll, and when the interface is re-enabled.
- **When hover runs.** Only while the in-world cursor is shown over the map window. It does not run during NPC combat turns, when the game mouse is disabled.
- **Object under the pointer.** No object is found while the pointer is over a visible roof, unless the pointer is on a nonzero pixel of the player cutout mask (section 3.37). Otherwise the candidates are the objects with a nonzero art pixel under the pointer. A candidate with any translucency bit `0x4000`-`0x80000` other than "explicitly no translucency" `0x8000` is never solid. A wall or scenery candidate with none of those bits is "inside the cutout", and never solid, when it passes the section 3.37 orientation table **without** the wall-transparency-end exceptions (and whether or not the player is hidden) and the cutout mask pixel under the pointer is nonzero. The topmost solid candidate that is not dead or knocked out wins, falling back to the bottom-most candidate.

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
  - It is ` X ` in red when the attack is impossible: dead target, the crippled-arm conditions, not enough AP, out of range, an empty magazine, a blocked ranged shot, or no valid attack (SPEC-007 §3.2). It is also ` X ` over every critter while the active slot reports no attack, as in reload mode (section 3.23, "Attack query").
  - Without a critter under the pointer the plain crosshair is shown.

**Clicks** (left release in the map area, y at most 379). Every left-button map action fires on **release**, at the release position; the press itself does nothing. A release before the 250-millisecond hold pulse is an ordinary click. A right click acts on press. `[Must-match]`

- **Movement.** `[Must-match]` A movement click is handled in this order:
  1. **Target hex.** The hex under the pointer is found. When the pointer is outside the hex grid, nothing happens and nothing is cancelled.
  2. **Budget or cancel.**
     - In combat the step budget is current AP plus remaining free movement. With Ctrl held, the current attack's AP cost (section 3.23) is subtracted first. When the result is 0 or less, nothing happens.
     - Out of combat there is no budget. When `fallout.cfg` [system] `interrupt_walk` is on (default 1, read at every click), the player's current sequence is cancelled at this point, whatever it is doing ("Clicks while the player is busy", below). The cancel comes before the new walk is registered and before any route is searched. It does not depend on whether the new walk can be made.
  3. **Walk or run.** The click walks, or runs when the `running` preference is on. Shift inverts the preference. A run request clears sneaking unless the player has Silent Running. This happens before the walk is registered, so it happens even when the walk then fails or finds no route. A run request walks instead in the cases of SPEC-007 §3.11.1.
  4. **Registration.** A sequence holding one walk or run entry to the target hex is registered. It fails and nothing more happens in two cases: the player still has a running sequence (the cancel was not made, or a prioritized sequence refused it, SPEC-007 §3.10.1), or the in-combat budget is 0. When the target is the player's own hex, the sequence holds no entry and ends at once.
  5. **Route.** The entry starts at once and only then searches its route from the player's hex (SPEC-007 §3.1, goal not strict). Neither the click nor the registration tests the route beforehand.
- **No route.** `[Must-match]` When the route search returns no route, the entry fails as it starts. The sequence ends (SPEC-007 §3.10.1) and the player is set to the standing pose. Nothing is printed, no sound plays and no AP are spent. The hover's red `X` (above) predicts this case, but it does not prevent the click.
  - **Out of combat**, the cancel in step 2 has already ended the old walk. A no-route click therefore stops the player where the cancel left it: standing on the hex the player occupies, which can lag the drawn sprite by up to one hex (SPEC-007 §3.11.1). The old path is not resumed.
  - Clicking the player's own hex stops a walking player the same way.
  - Other ways to get no route: a target beyond a closed door, since the player never opens doors on a route (SPEC-007 §3.1); a target the search cannot reach within its capacity; an enclosed target; and an occupied target one step away, whose route loses its last step and is left empty.
  - This stop is reference behavior, not a defect. It is the only way to stop a walk without starting another.
- **In combat** the player is never busy when a click is accepted, so nothing is cancelled. The registration takes the combat input lock (section 3.27) for as long as the sequence runs. A no-route or own-hex click takes the lock and releases it within the same click, so no watch cursor is seen and the hover is recomputed on the next idle poll. The route is not cut to the budget in advance: the walk ends when AP plus free movement reach 0 as AP are charged per hex (SPEC-007 §3.11.1). A click on a hex whose hover shows the red `X` for lack of AP therefore still walks as far as the AP allow.
- **Arrow.** Perform the primary action:
  - item: pick up;
  - player: rotate, only when standing;
  - talkable critter: talk out of combat, examine in combat;
  - other critter: loot;
  - scenery: use if usable, else examine;
  - wall: examine.
- **Crosshair.** Attack the critter under the pointer: preflight, combat start when out of combat, and the called-shot window in an aimed mode, as in section 3.38. The hover is then marked done at the click position, so the to-hit text stays as it was until the pointer moves. It is recomputed by force only at the start of each player combat turn and when the in-world cursor is turned back on.
- **Use crosshair.** In combat a non-weapon needs and spends 2 AP; when the AP cannot be paid nothing happens and no message is shown. Otherwise use the active item on the target. The mode returns to movement in every case, even with no target.
- **Skill modes.** As SPEC-013 §3.1.

**Clicks while the player is busy.** `[Must-match]` unless tagged. Busy means that a sequence holding the player is still playing, such as an approach, a reach, a put-away, a take-out or a door use. Nothing is ever queued. A click either cancels the current sequence and starts its own, or does nothing. The rules come from SPEC-007 §3.10.1 ("One sequence per owner").

- **In combat**, player input is locked by a combat-wide count of running sequences and by the turn order. `[Must-match]` unless tagged.
  - **Sequence count.** Every sequence that registers while combat is active adds 1 to the count, whatever it holds: the player, an NPC, a door, a projectile, or an extra victim's reaction. It subtracts 1 when it ends, whether normally, by failing while it plays, or by being cancelled. A sequence whose registration fails never counts, and neither does a sequence registered before combat began.
  - **Lock during the player's turn.** When the count rises from 0 to 1 during the player's turn, the game mouse, the keyboard and the interface bar are disabled, with edge scrolling left on, and the cursor becomes the animated watch `wait2.frm` (list index 295). When the count falls back to 0 during the player's turn, they are re-enabled and the in-world cursor returns. "Until every running sequence has ended" therefore means until this count is 0. A sequence that starts while another is still running, such as a door the player opens or an extra victim's reaction, extends the lock.
  - **Lock between player turns.** Combat start disables the same three and shows the animated watch. At the start of each player turn they are re-enabled and the in-world cursor returns. When the player's turn ends, after the count has returned to 0, they are disabled again and the animated watch is shown. It stays through every NPC turn until the next player turn begins. Sequences registered during an NPC's turn change the count but not the cursor or the lock. The end of combat re-enables everything. The animated watch therefore shows from combat start to the player's first turn, during the player's own sequences, and on every NPC turn. Only edge scrolling replaces it, and then only for a moment (section 3.29).
  - **Mouse.** Map clicks made while locked are discarded (section 3.26), and the disabled bar buttons ignore clicks. The Swap hands button is never disabled. During the lock it still shows its pressed art and plays its sounds, but the `b` it sends does nothing because the bar is disabled. `[May-improve]`: disable the Swap hands button with the other bar buttons.
  - **Keyboard.** No key is read while the keyboard is disabled, so no key works during the lock: not Ctrl+Q, Ctrl+X, F10, Ctrl+P, F6, F7, F12, the arrow keys, Space, Enter or any other key. The key presses and releases are not discarded. Up to 32 of them are held and are read in order once the keyboard is enabled again.
    - Keys typed during one of the player's own sequences, while the player still has AP, therefore take effect when the sequence ends.
    - At the start of each player turn, before the player's combat script runs and before any automatic stand-up, held keys are flushed. Keys typed at combat start, during NPC turns, or during the last sequence of the player's previous turn are therefore lost.
    - The end of combat re-enables the keyboard without a flush. Keys typed during the last turns of a combat are read after it ends.
    - `[May-improve]`: which key events survive when more than 32 are typed during one lock depends on the reference's input library. Safe rule: keep the first 32.
- **Movement click, out of combat.**
  - With `interrupt_walk` on, the click cancels the player's current sequence before the new walk is registered or its route searched ("Movement", above). The new walk then starts. When it has no route, the player is left standing instead. A reach or container use in progress is abandoned. Its unstarted entries are dropped: the use itself, the weapon take-out and the loot request.
  - A sequence the player is not part of keeps playing. An example is a door's opening animation that the use already started.
  - With `interrupt_walk` off, the new walk cannot register while the player is busy, so the click does nothing.
- **Arrow click or action-menu entry** (pick up, use, loot, talk, skill use).
  - If the player's current art is walk or run, the current sequence is cancelled first. A click during the approach of an earlier action therefore replaces that whole action, not only its movement.
  - In any other pose, such as a reach, put-away, take-out or standing inside a sequence, nothing is cancelled. The new action's first player-owned entry cannot register, so the click does nothing and shows no message.
  - Talk tests the object distance (below 9) and the line from the player's hex to the target's hex, which must not be shot-blocked (SPEC-007 §3.2, target ignored). It measures both twice, each time from the player's position at that moment. `[Must-match]`
    - The first test is made when the click or action-menu choice is handled, and only out of combat. It decides whether there is an approach. When either test fails, a run to the target is registered first (SPEC-007 §3.11.1 gives when it walks instead). In combat there is always an approach: a walk to the target limited to the player's current AP.
    - The second test is a check entry that runs after the approach, or at once when there is none. A failure prints `misc.msg` 2000 "You cannot get there." and ends the sequence, so no dialogue is requested. On success the dialogue request is made in the same examination.
    - Without an approach the sequence consists only of callbacks, so it registers even while the player is busy. Its dialogue request is then made at once, while the earlier animation plays.
    - The check entry is forced (SPEC-007 §3.10.1). When the sequence ends before the check has run, for example because a new walk cancels the approach, the check runs at that moment. It prints message 2000 when the player is not yet within range or the line is blocked, and the dialogue is never requested either way. `[May-improve]`: print nothing when the sequence is cancelled before its check runs.
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
- **Arrow keys** scroll one step per key event through the same 33-millisecond gate. They use the key auto-repeat below. Arrow keys work regardless of the edge-scroll enable state, but not while the keyboard is disabled. `[Must-match]`
- **Key auto-repeat.** `[Must-match]`
  - Repeat is produced by the game, not the host. It applies to every physical key alike, with no exemption for window toggles.
  - Each time input is polled and the keyboard is enabled, every key that is still held makes one more key event of the same kind. That happens when more than 500 milliseconds have passed since its press, or more than 80 milliseconds since its previous repeat.
  - Repeats only happen during polls, so a loop that polls less often repeats less often.
  - F1, Tab, Esc, `o`, `i`, `c`, `p` and the other toggles therefore repeat when held. A held toggle opens its window. After about half a second its repeats are read by the open window, which closes it if it treats that key as close. Its next repeat then reopens the window from the game loop, and so on until the key is released. The help screen (F1) closes on any input, so its own repeat closes it.
  - **Modifier keys repeat too, but produce no key.** Shift, Ctrl, Alt, Caps Lock, Num Lock and Scroll Lock are physical keys like any other. They are timed and repeated by the same rule, and each press or repeat is placed in the 32-event keyboard buffer (section 3.27). A buffered event is translated to a key code only when it is read, using the modifiers held at the time of the event. A modifier key on its own translates to no key code, so its press or repeat is removed from the buffer and nothing acts: no window opens or closes, and no "any key" test is met. Such an event still uses one buffer slot and one poll. Holding Ctrl therefore never closes the pause window that Ctrl+P opened. Holding P while Ctrl is held repeats Ctrl+P like any other key. `[Must-match]`
  - Clearing the keyboard cancels the repeat of every held key until that key is pressed again. This happens at the start of each player combat turn (section 3.27) and at the top of each main-menu loop pass (section 3.4).
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

**Pip-Boy automap page.** This page draws from the saved automap database (SPEC-008), not from live objects. It is current only up to the last database write: leaving the map, saving the game, or taking an elevator. Each stored cell paints 2 pixels wide and 1 pixel tall, on a 2-row vertical pitch: cell (c, r) of the raster paints window pixels (238 + 2c, 105 + 2r) and (239 + 2c, 105 + 2r), and the row between two cell rows is never painted, so the page shows every other line. This matches the two-row pitch in SPEC-008 §2.8: the 200 cell rows span 400 window rows, but only the first of each pair is drawn. Wall cells use key 992 and scenery cells key 480. The player, critters and items are not drawn. Rows from 188 on would fall below the 480-pixel window; R1 relies on those rows being empty. `[Must-match]` `[May-improve]`: clip cells to the window.

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

1. **List area**, x 124-188 and y 35 through the bottom of the visible rows. The row is `trunc((y - 39) / 48)` although rows start at y 35, the same 4-pixel offset as SPEC-013 §3.16: the top 4 pixels of rows 1-5 drop into the row above. `[May-improve]`: use `trunc((y - 35) / 48)`, as in SPEC-013 §3.16.
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
5. If the map number at the player's elevation is in the excluded set, or the map has no number (SPEC-008 §3.10), delete any working `.SAV` for it and write nothing. The excluded set is SPEC-008 §2.8's initial set: maps 0-2, 19-23, 49-54 and 56-65, which are the desert and encounter maps. Those maps are rebuilt from their `.MAP` on every visit. Otherwise write `MAPS\<stem>.SAV`, named by SPEC-008 §3.10 "Working `.SAV` names", and capture the automap for that elevation.
6. Remove every object, the prototype cache and the tile data.

**Loading a map.** `[Must-match]`

- The name comes from `map.msg` by map number and is uppercased. Probe `MAPS\` plus that name with its first `.MAP` replaced by `.SAV`, for example `MAPS\VAULTENT.SAV` for map 7. Loads by file name probe the same way (SPEC-008 §3.10).
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
- Leaving a map (section 3.35 step 3): if the last square, read on the player's current elevation, is not empty, run the restore fill from it before the map is written. Every player placement records its elevation (step 4), so the player's current elevation is always the last recorded one. This step therefore restores the group hidden at the last square, whether or not the elevation-change safe rule below is applied. A group left hidden on another elevation by the elevation-change defect is written still hidden, and it comes back drawn on the next load (first bullet). A game save does not leave the map and restores no roof: hidden roofs are written hidden and come back drawn when that save is loaded.

**Defects.**

- The first placement after a reset reads the "previous" nibble at position `-1 + 100 × -1`, 101 words before the start of the grid. With last-empty still true, the value read never changes the result. `[May-improve]`: when there is no last square, skip the nibble comparison and test only E against last-empty.
- On an elevation change, step 2 reads p and step 3a restores on the **new** elevation, at the old square's position. A group hidden on the old elevation therefore stays hidden there. It reappears only when the player later enters and leaves it on that elevation, or when the map is reloaded. A matching group on the new elevation at the old position, if hidden, is restored instead. `[May-improve]`: run the restore fill on the elevation where the group was hidden, and take p from that elevation.

### 3.37 Player See-Through Cutout

Roofs, and walls and scenery that would hide the player, are drawn partly transparent inside an elliptical "egg" around the player. Nothing else uses the cutout. `[Must-match]` unless tagged.

**Mask.** `egg.frm`, `intrface.lst` index 2: one direction-independent frame, 129 by 98, with all six direction shifts (0, 25) and frame offset (0, 0). Each pixel value m is a mask weight. m = 0 lies outside an ellipse centered on mask column 64, between rows 48 and 49; inside it the flat core is m = 1 (4716 pixels), rising toward the rim to a maximum of 122. The mask itself is never drawn.

**Position.** The mask follows the player. It is placed on the player's hex every time the player is placed on a hex, which resets its displacement to 0, and it receives every pixel displacement the player receives while animating. Its rectangle uses the ordinary world-object placement (SPEC-001 frame anchor, SPEC-003 §2.10) with direction 0 and frame 0: with the player hex's projected center (cx, cy), meaning its hex screen position plus (16, 8), and the player's current displacement (dx, dy), the mask covers screen x `cx + dx - 64` through `cx + dx + 64` and y `cy + dy - 72` through `cy + dy + 25`, inclusive. Mask pixel (u, v) lies at `(cx + dx - 64 + u, cy + dy - 72 + v)`. Elevation plays no part.

**Roofs.** Every drawn roof square (not hidden, art index not 1, section 3.36) is drawn through the mask at the ambient light level, whether or not the player is hidden and whatever the square's position relative to the player.

**Walls and scenery.** An object whose art type is scenery (2) or wall (3) is drawn through the mask only when all of these hold:

1. The player is not hidden.
2. The object has none of the basic translucency bits `0x4000`, `0x8000`, `0x10000`, `0x20000`, `0x40000`, `0x80000` (SPEC-003 §2.10). "Explicitly no translucency" therefore also excludes it.
3. The orientation test below passes.

For the orientation test let (dx, dy) be the object's hex screen position minus the player's, as integers, and evaluate in IEEE-754 double precision with K the double `0x3FF5555555555556` (1.3333333333333335; not the nearest double to 4/3, and the difference decides real cases at equality):

- R: `dx <= dy × K`.
- R′: `dx >= dy × K`.
- F: `dx >= -4 × dy`.
- F′: `dx <= -4 × dy`.

Let END be the object's basic flag `0x10000000` (wall transparency end marker). The first row matching the object **prototype's** extended flags applies:

| Prototype extended flags | Cut when |
| --- | --- |
| `0x08000000` or `0x80000000` set | F and not (R′ and END) |
| else `0x10000000` set | F or R |
| else `0x20000000` set | F and R |
| otherwise, including `0x40000000` or none of these | R and not (F′ and END) |

When the cut applies and the mask rectangle overlaps the object's clipped screen rectangle, only the overlap is drawn through the mask; the up to four strips of the object rectangle outside it use the normal lit draw. Otherwise the object uses its normal translucent or lit draw (section 3.44).

**Compositing.** For each pixel of the masked area, with L the light level divided by 512 (0-128) and the intensity and additive-mix tables of SPEC-001:

- Source index 0 leaves the destination unchanged.
- Otherwise s = intensity[source][L]. With mask weight m = 0 the result is s; otherwise the result is mix[intensity[s][m]][intensity[destination][128 - m]].

So m/128 is the drawn object's opacity: the core (m = 1) shows almost only what was drawn beneath, and opacity rises toward the rim. The normal lit draw leaves source indexes 229-255 unlit; the masked draw lights them as well, even where m = 0. `[May-improve]`: apply the same 229-255 exemption in the masked draw.

**Failures.** When the mask art cannot be loaded, the reference draws neither the roof square nor the object whose cut applied. When the player has no hex, the mask position is undefined. `[May-improve]`: in both cases draw normally, without a cutout.

### 3.38 Called-Shot Window

The called-shot window lets the player choose one of eight body parts for an aimed attack. Only the player uses it; NPCs choose called locations through the AI (SPEC-009). `[Must-match]` unless tagged.

**From click to window.** A crosshair click on a critter (section 3.27) makes the player's attack request against it:

1. Nothing happens unless the player-input gate bit `0x02` is set (SPEC-007 §2.4) and the active slot reports an attack (section 3.23, "Attack query"). Reload mode reports none, so a click then does nothing.
2. The attack preflight of SPEC-007 §3.2 runs with the slot's hit mode and aimed flag. The first failing test, in this order, prints its `combat.msg` line and opens no window:

   | Test | Message |
   | --- | --- |
   | Target dead | None |
   | Weapon in use and both arms crippled | 105 "You cannot use weapons with both arms crippled." |
   | Weapon in use, one arm crippled, two-handed weapon | 106 "You cannot use two-handed weapons with a crippled arm." |
   | AP below the attack cost (aimed cost in an aimed mode, SPEC-007 §3.4) | 100 "You need %d action points.", with that cost |
   | Distance beyond range | 102 "Target out of range." |
   | Magazine empty | 101 "Out of ammo.", plus the weapon's out-of-ammo sound (effect `O`, SPEC-010) |
   | Ranged or thrown shot blocked | 104 "Your aim is blocked." |

3. **Out of combat**, a passing click starts combat with the player as attacker and the critter as defender (SPEC-007 §3.3). No window opens yet. The player's first turn refills AP and stands the player up if prone, then re-enables the interface. The click is then replayed once: steps 1 and 2 run again with the refilled AP, then step 4.
4. **In combat**, a mode that is not aimed attacks at once with the uncalled location 8. An aimed mode opens the window.

**Window.** A modal window at screen (108, 20), 424 by 309, horizontally centered. Windows beneath it get no button input, and the interface bar (from y = 379) stays uncovered. The background is `called.frm` (list index 118, 424 by 309, one frame), copied opaquely. Its art already contains the frame, the eight chance boxes, the leader lines and the word CANCEL. No other background art is used.

**Critter picture.** The picture is the critter art with animation code 64 (suffix `na`, SPEC-007 §2.2) for the target's current art base index. It uses weapon code 0 and file selector 0, and goes through the critter-list alias, which applies to code 64 (SPEC-007 §2.1). The file is therefore `<alias stem>na.frm`. Frame 0 is copied opaquely, index 0 pixels included, as 170 by 225 pixels at (128, 31). It is not scaled or animated, and the target's facing, pose and weapon have no effect. When the file is missing, that area keeps the background and the window still opens. All 21 corpus `na` files are 170 by 225 with one frame. The reference copies 170 by 225 with a 170-pixel row stride whatever the frame's real size. `[May-improve]`: copy only the frame's own pixels, clipped to 170 by 225.

**Rows.** Four rows with tops y = 36, 102, 166 and 230. Each row has a left and a right body part. A part's button index is its selection code.

| Row | Top y | Left part (code) | Right part (code) |
| ---: | ---: | --- | --- |
| 0 | 36 | Head, location 0 (code 0) | Torso, location 3 (code 4) |
| 1 | 102 | Eyes, location 6 (code 1) | Groin, location 7 (code 5) |
| 2 | 166 | Right arm, location 2 (code 2) | Left arm, location 1 (code 6) |
| 3 | 230 | Right leg, location 4 (code 3) | Left leg, location 5 (code 7) |

Each row is drawn in this order: left chance, left label, right chance, right label. In each part:

- **Chance.** The section 3.5 range-aware hit chance of SPEC-007, for the player against the target with the slot's hit mode and this location. The location adjustment is the full value for ranged and thrown attacks and half for melee and unarmed (SPEC-007 §2.4). It is computed once, when the window opens. The digits come from the white band of `numbers.frm` (list index 82, 360 by 17; section 3.21): the tens digit at (x, y) and the ones digit at (x + 9, y), each a 9-by-17 cell from source x = 9d, copied opaquely. Here x = 33 on the left and 373 on the right, and y is the row top.
  - 0 through 9 show a leading zero, for example `05`.
  - The cap of SPEC-007 §3.5 keeps every chance at 95 or below.
  - A negative chance draws the 6-by-17 minus glyph (source x = 108) at (x, y) and again at (x + 9, y). Columns x + 6 through x + 8 and x + 15 through x + 17 keep the background, so every negative chance looks the same.
- **Label.** The text of `combat.msg` message 1000 + 10 × A + L, where L is the location number and A is the alias of the target's current art base index in `critters.lst` (SPEC-007 §2.1). The picture uses the same alias. Font 101 draws the label over the art without a background fill, in key 992 [215]. A left label starts at (74, y). A right label of width w ends at x = 350: it is drawn at (351 - w, y). When the message is missing, no label is drawn.
- **Button.** An invisible 88-by-10 rectangle with no art and no sound. On the left it is at (33, y + 4); on the right at (351 - w, y + 4), with w = 10 when the label is missing. The rectangle does not follow the text:
  - it starts 4 rows below the label top;
  - on the left it covers the chance digits and only label columns 74 through 120, so the end of a longer label is not clickable;
  - on the right it runs 88 pixels from the label's left edge and may cover the right chance digits.

**Hover.** When the pointer enters a part's rectangle, its label is redrawn at once in red, key 31744 [133]. When the pointer leaves, it is redrawn in green, key 992 [215]. Nothing else changes and no sound plays. Each redraw goes over the previous text without restoring the background, so the partial-coverage pixels of the font (SPEC-002 §3.2) blend with the earlier label color. `[May-improve]`: restore the label's background before each redraw.

**Cancel.** A Red button at (170, 268): `lilredup.frm` / `lilreddn.frm` (list indexes 8 and 9), drawn transparently over the art. Its release returns Esc.

**Input.** The window polls input until one of these ends it:

- **Esc**, by key or by the Cancel button, closes the window and does nothing else. No attack is made, no AP are spent, no message prints, and no sound plays apart from the Red button's own sounds. The cursor stays in crosshair mode. Out of combat (step 3) combat has already started, and the player's turn continues with full AP.
- **A left click on a part's rectangle**, pressed and released on the same rectangle, chooses that part. Right clicks do nothing.
- **Input codes 1-7** choose by selection code, like the buttons. The keyboard produces them for Ctrl+A through Ctrl+G, by the letter of the current layout. Ctrl+A chooses the eyes, Ctrl+B the right arm, Ctrl+C the right leg, Ctrl+D the torso, Ctrl+E the groin, Ctrl+F the left arm and Ctrl+G the left leg. The digit keys 1-8 do nothing, and there are no other hotkeys. All other keys and clicks are discarded.
- **Input code 8**, from Backspace or Ctrl+H, is also accepted as a choice. It reads past the end of the right-hand location table, so the location is undefined. `[May-improve]`: ignore code 8 and keep the window open.
- **A pending quit request** (for example a script's map or world-map request during combat) ends the loop and takes the last input code as the choice. That code is normally "no input", which reads before the start of the left-hand table. The sound then plays and the attack runs with an undefined location. `[May-improve]`: treat a pending quit request as Esc.

While the window is open:

- the system cursor is the arrow;
- the in-world cursor is hidden, and edge scrolling is off when the game mouse was on at entry;
- an interface that was disabled is enabled, then disabled again when the window closes;
- map background processing keeps running, unlike in the action menu (section 3.28). That covers object animation, idle fidgets, floating text, palette cycling and running scripts.

On close the game mouse is turned back on with edge scrolling enabled.

**Result.** When a part is chosen:

1. The window closes and `icsxxxx1` plays.
2. The player's attack request runs with the slot's hit mode and the chosen location 0-7 (SPEC-007 §3.6 and §3.10).
3. The attack is debited at the aimed cost, the weapon or punch cost plus 1 after reductions, once the attack is done (SPEC-007 §3.4). The preflight has already confirmed that cost.

Choosing the torso requests location 3, not the uncalled location 8. Its accuracy adjustment is the same 0, but it still pays the aimed cost. Afterwards the crosshair hover is recomputed as for any crosshair click (section 3.27).

**Random draws.** Neither the window nor its chance figures draw from the shared generator. Every draw of the attack request comes after the choice, starting with the punch-to-kick draw for a punch (SPEC-007 §3.6). Esc draws nothing.

**Failures.** When the window cannot be created, or the background or either Cancel art cannot be loaded, the window closes at once and the result is the same as Esc. A missing picture or label leaves out only that element. `[Must-match]`

**Body types.** Every critter gets the same layout, the same eight parts and the same hit-chance rules. Only the picture and the labels change, and both are chosen by the art alias. Prototype kill type and body type play no part. All eight parts can always be chosen, even where the labels repeat or read `???`.

The table lists every alias in the corpus `critters.lst`. Every label is prefixed with "the "; it is omitted in the table, and "the head" is shown as head. Locations are given in order 0-7: head, left arm, right arm, torso, right leg, left leg, eyes, groin.

| Alias row (stem) | Messages | Rows that use it | Labels for locations 0-7 |
| --- | --- | --- | --- |
| 11 (`hmjmps`) | 1110-1117 | 2-14, 26, 28, 29, 31-41, 43-45, 47, 48, 56, 57 | head, left arm, right arm, torso, right leg, left leg, eyes, groin |
| 15 (`mabrom`) | 1150-1157 | 15 | head, left hindleg, right hindleg, torso, right foreleg, left foreleg, eyes, udder |
| 16 (`maddog`) | 1160-1167 | 16 | head, hindleg, hindleg, body, foreleg, foreleg, eyes, groin |
| 17 (`mahand`) | 1170-1177 | 17 | CPU, manipulator, manipulator, frame, motivator, motivator, sensors, regulator |
| 18 (`malieu`) | 1180-1187 | 18 | As alias 11 |
| 19 (`mamrat`) | 1190-1197 | 19 | head, left paw, right paw, body, right leg, left leg, eyes, groin |
| 21 (`mamtnt`) | 1210-1217 | 1, 20, 21, 30 | As alias 11 |
| 22 (`mascrp`) | 1220-1227 | 22 | head, tail, claw, carapace, forelegs, hindlegs, eyes, brain |
| 23 (`masphn`) | 1230-1237 | 23 | head, mouth, fore mouth, body, float sac, tail, ganglion, ovipositor |
| 24 (`masrat`) | 1240-1247 | 24 | As alias 19 |
| 25 (`mathng`) | 1250-1257 | 25 | head, left arm, right arm, body, right leg, left leg, eyes, groin |
| 27 (`nachld`) | 1270-1277 | 27, 42 | As alias 11 |
| 46 (`nmover`) | 1460-1467 | 46 | As alias 11 |
| 49 (`nmgzmo`) | 1490-1497 | 49 | As alias 11 |
| 50 (`mamstr`) | 1500-1507 | 50 | head, ???, ???, frame, ???, ???, eyes, CPU |
| 51 (`maclaw`) | 1510-1517 | 51 | As alias 11 |
| 52 (`mamant`) | 1520-1527 | 52 | head, raptorial leg, raptorial leg, abdomen, legs, wings, eyes, metathorax |
| 53 (`marobo`) | 1530-1537 | 53 | As alias 17 |
| 54 (`mafeye`) | 1540-1547 | 54 | As alias 17 |
| 55 (`mamurt`) | 1550-1557 | 55 | As alias 19 |
| 58 (`madegg`) | 1580-1587 | 58 | shell, all eight |

"Row" is the zero-based `critters.lst` line, which is the art base index.

- Row 0 (`reserv`) has no alias field. It takes the default alias 0: messages 1000-1007 and `reservna.frm` do not exist, so the window shows no labels and no picture.
- The female player and every female human or ghoul row alias to `hmjmps`, so the window shows the male human picture.
- The player's picture follows the current armor art; every player armor row aliases to 11 or 21.

### 3.39 World Map Screen

SPEC-008 owns the rules behind this screen: world coordinates, the travel step, travel time and healing (§3.4), discovery and encounters (§3.5), the lookup grids (§3.6), town cells, knowledge, target jitter and the hotspot catalog (§3.7), and map numbers and music (§3.10). This section adds the screen itself: its art, drawing, scrolling, markers, side panel and input, and what each exit leads to. The town map is in section 3.40. Coordinates are window coordinates; the window is at screen (0, 0), so they are also screen coordinates.

**Opening.** `[Must-match]`

- The world map opens:
  - from an exit grid holding map 0, and after a town map opened by an exit grid holding map -1 (SPEC-013 §3.22);
  - from the Town/World button of a town map (section 3.40);
  - from the script opcodes below.
- First the current map is saved into the working map set (section 3.35). If that save fails, the dialog and return to the main menu of SPEC-013 §3.22 follow. A pending quit request makes the screen return at once.
- The window is 640 by 480 at screen (0, 0) and covers the game view and the interface bar. The indicator boxes (section 3.24) are switched off and switched on again when a map is loaded from this screen.
- Map background processing and palette cycling are suspended while the screen is open. The text font is 101 and the cursor the arrow.
- The party's world position, discovery grid and known towns are kept from before. Opening the screen again from a town's map therefore shows the party at rest on that town. The only exception is a town-map choice that starts a journey, described under "Town-map results" below.
- The world-map music `03wrldmp` (SPEC-008 §3.10) starts from its beginning each time the screen is built. That happens on opening and again on every return from a town map.
- The speed values W and Q of SPEC-008 §3.4 are computed once, at opening.

**Script requests.** `[Must-match]` The opcode `world_map` (`0x8108`) first opens the town map of the current town and then the world map with that town map's result, exactly like an exit grid holding map -1. The opcode `town_map` (`0x8109`) opens the world map directly, without a town map. The main loop serves both requests out of combat. A request made during combat ends combat first. The current town is the last town entered through this screen or a town map. It starts as Vault 13 (SPEC-008 §3.4), and neither map changes nor script-directed travel change it.

**Art.** All of it is in `art/intrface/`. Sizes are from the corpus FRM headers, and each file has one frame. `[Must-match]`

| List index | Art | Size | Use |
| ---: | --- | --- | --- |
| 135 | `worldmap.frm` | 1400 by 1500 | The whole world as one image. There are no tiles |
| 136 | `wmapbox.frm` | 640 by 480 | Frame. It has a transparent hole at (22, 21), 450 by 442, with rounded corners, and painted plates for the date, the 12 town lamps and the Town/World button |
| 137 | `wmaplabs.frm` | 82 by 216 | Twelve town-name plates of 18 rows each, in town-id order (`VAULT 13` through `CATHEDRAL`). The names are art, not message text |
| 138 | `wmaploc.frm` | 5 by 5 | Party while travelling: a red plus sign |
| 139 | `wmaptarg.frm` | 11 by 11 | Destination crosshair, bright red |
| 153 | `wmaptrg2.frm` | 11 by 11 | The same crosshair in dark red, used for the special-encounter flash |
| 154 | `wmapfgt0.frm` | 7 by 11 | Random-encounter lightning bolt, bright |
| 155 | `wmapfgt1.frm` | 7 by 11 | The same bolt, dark |
| 168 | `hotspot1.frm` | 25 by 13 | Party at rest: a hollow green triangle pointing down |
| 223 | `hotspot2.frm` | 25 by 13 | The same triangle filled, shown while entering a location |
| 129 | `months.frm` | 29 by 179 | Month names, one per 15 rows |
| 82 | `numbers.frm` | 360 by 17 | Digits. Only the white band is used (section 3.21) |
| 8, 9 | `lilredup.frm`, `lilreddn.frm` | 15 by 16 | Red buttons, up and down |

No other art is drawn. The map shows no town circles and no town names, and there is no vehicle marker: R1 has no car. `tmapcirc.frm` (43 by 43) exists in the art directory, but it is not in `intrface.lst` and nothing uses it. `[Must-match]`

**Layout.** `[Must-match]`

| Element | Position and size | Notes |
| --- | --- | --- |
| World view | (22, 21), 450 by 442 | The frame's hole. View origin (vx, vy) is the world pixel shown at (22, 21), clamped to 0-950 and 0-1058 (SPEC-008 §3.4) |
| Day | (487, 12), 2 digits | Each digit cell is 9 by 17, written opaque, with leading zeros. Digit d comes from `numbers.frm` x = 9d |
| Month | (513, 12), 29 by 14 | Rows `15 (m - 1)` through `15 (m - 1) + 13` of `months.frm`, written opaque |
| Year | (548, 12), 4 digits | |
| Time | (593, 12), 4 digits | HHMM, as `game_time_hour` (SPEC-005) |
| Town buttons | (508, Y), 15 by 15 | One per town t = 0-11, with Y = 61, 88, 115, 143, 171, 200, 228, 256, 283, 310, 338, 367. Red button art; only its top 15 rows are shown. Sounds `ib1p1xx1` on press and `ib1lu1x1` on release; the event comes on release |
| Town names | (531, Y), 82 by 18 | Plate t of `wmaplabs.frm`, drawn with transparency for every town whose known/visited bit (SPEC-008 §2.7) is set |
| Town/World | (520, 439), 15 by 15 | Red button with the same sounds, on the frame's painted `TOWN / WORLD` plate |

The date is month, day and year from the game clock, using the calendar of SPEC-005. All 12 town buttons are always present and always clickable. A town that is not known has an empty plate and its button does nothing. The town names are drawn only when the screen is built, so they do not change while it stays open. `[Must-match]`

**Window position of a world pixel.** World pixel (x, y) appears at `(x - vx + 22, y - vy + 21)`. Let (px, py) be the party's window position. Centering on the party sets `vx = X - 247` and `vy = Y - 242`, clamped, where (X, Y) is the party's world position. An unclamped centered party therefore appears at (269, 263), which is 22 pixels right of and 21 below the view's center. `[Must-match]`

**Follow mode.** `[Must-match]`

- Follow mode is on when the screen opens. While it is on, every loop pass centers the view on the party and redraws.
- `A` or `a` toggles it.
- Edge scrolling, an arrow key, Home and End switch it off. An edge position switches it off even when the view cannot move.
- A destination click or a town-button journey switches it on when the party was at rest and off when the party was already travelling.
- When the World button returns from a town map, it is off if the town map was opened with the Town/World button and on if the town map was opened by entering a town. A journey chosen on a town map switches it on.
- `C`, `c` and Home center the view once, even while follow mode is off.

**Scrolling.** `[Must-match]` unless tagged.

- **Edges.** Scrolling starts when the pointer is at x 0-1 (west), x 638-639 (east), y 0-1 (north) or y 478-479 (south); corners scroll diagonally. These edges are 2 pixels wide, unlike the in-game edges of section 3.29.
- **Step.** Each loop pass moves 16 pixels on each requested axis. Each axis is clamped on its own, so a diagonal against one limit still slides along the other axis.
- **Cursor.** It is set on every pass. The pointer shows the direction's scroll art (section 3.26), or the blocked art when every requested axis is already at its limit. With one axis of a diagonal blocked, the normal diagonal art shows. Away from the edges it shows the arrow.
- **Arrow keys.** Each key event moves 16 pixels. A key is ignored when the view is already at that limit. On that axis, for that pass, the key replaces any direction from the edge.
- **End** sets vy to 1058; vx is unchanged.
- **Home** first centers both axes on the party, then sets vy to 0 and applies a 16-pixel southward step, so vy ends at 16. `[May-improve]`: leave vy at 0.
- **Speed.** A pass that redraws lasts at least `trunc(1000 / 24)` = 41 milliseconds, measured from the start of the pass, so continuous scrolling moves about 16 pixels every 41 milliseconds.

**Drawing a pass.** These steps happen whenever anything changed: follow mode, a scroll, a change of the hover state below, a travel iteration, or a state change. `[Must-match]`

1. Copy the world art's 450-by-442 rectangle at (vx, vy) to (22, 21).
2. Darken the view by discovery cell (SPEC-008 §2.7, §3.5). Cell (c, r) covers world x `50c` through `50c + 49` and y `50r` through `50r + 49`, clipped to the view:
   - Value 0: fill the cell with key 0 [228] (black).
   - Value 1: replace each pixel index P that is neither 0 nor 229-255 by intensity-table entry (P, 64) (SPEC-001 §2.1). When the table is generated (SPEC-002 §3.2, K below 128), this halves each 5-bit RGB component, `floor(C / 2)`, and maps the result back through the color lookup. With the corpus palette, for example, 50 becomes 52, 150 becomes 153 and 100 becomes 228. The corpus world art has no pixels of index 0 or 229-255, so every pixel of a seen cell is darkened.
   - Value 2, or any other value: leave the cell unchanged.
3. Draw the party marker, with transparency:
   - At rest: `hotspot1.frm` at (px - 12, py - 6). Its tip is at (px, py + 6).
   - Travelling: `wmaploc.frm` at (px - 2, py - 2).
4. At rest, draw the hover label (below).
5. While travelling, draw `wmaptarg.frm` at the destination's window position minus (5, 5), then the travel dots (below).
6. Redraw the frame over the view's margins. Columns 0-34 and 455-486 are drawn with transparency over the full height. Rows 0-20 and rows 463-479 of columns 35-456 are copied opaque. The side panel, columns 487-639, is drawn only when the screen is built.
7. Draw the date and time, and present the window.

A pass that changes nothing only redraws the date and presents the window, without the 41-millisecond wait. Markers are skipped unless their top-left corner (x, y) lies in these ranges: `[Must-match]`

| Marker | x | y | Notes |
| --- | --- | --- | --- |
| Triangle | -2 through 483 | 9 through 462 | Width cut to `485 - x` when x is above 460 |
| Plus sign | 18 through 473 | 17 through 462 | |
| Crosshair | 12 through 473 | 11 through 462 | |
| Lightning bolt | 16 through 471 | 11 through 462 | |

Step 6 hides whatever lands on the frame. The exception is a triangle at x = -2 or -1, whose first columns land at x 638-639 of the row above. The side panel is not redrawn on each pass, so up to three stray triangle pixels stay at its right edge until the screen is next built. `[May-improve]`: clip all markers and labels to the view, (22, 21) through (471, 462). Step 6 also misses one corner pixel of the frame, (35, 462), which therefore shows world art. `[May-improve]`: draw the whole frame on every pass.

**Building the screen.** `[Must-match]` On opening and on each return from a town map, the screen is built in this order:

1. Apply the SPEC-008 §3.5 reveal once at the party's cell.
2. Draw the world art, darkening and the whole frame.
3. Draw the town names.
4. Draw the at-rest triangle, even when a journey is already under way.
5. Draw the date, and present the window.

Opening the world map therefore always marks the party's cell as visited.

**Travel dots.** `[Must-match]`

- One bit is kept per world pixel, 1400 by 1500.
- The party's current pixel is marked on the 1st, 4th, 7th and every later third travel iteration, at the same moment as the SPEC-008 §3.5 reveal. The count starts at 0 when the screen is built.
- While the party travels, every marked pixel inside the view is painted key 27648 [134] (dark red), over the markers.
- All marks are cleared as soon as travel stops, whether by arrival, the walking mask, an encounter or entering a location, and whenever the screen is built.
- A new destination clicked during travel keeps the existing marks and does not reset the count.

**Hover state.** `[Must-match]` On every pass, wherever the pointer (mx, my) is, let `dx = |mx - px|` and `dy = |my - (py - 1)|`. The smaller of the two is halved and truncated; dy is the one halved when they are equal. The pointer hovers on the party when `dx + dy` is at most 10. A change of the hover state triggers a redraw.

**Hover label.** It is shown only while the party is at rest and the pointer hovers. `[Must-match]`

- The text is chosen like this:
  - In the cell of town t: `map.msg` 500 + t (Vault 13, Vault 15, Shady Sands, Junktown, Raiders, Necropolis, Hub, Brotherhood, Military Base, Glow, Boneyard, Cathedral). The name is used when the town's known-town campaign value is 1 or its known/visited bit is set; otherwise the text is `worldmap.msg` 1004 "Unknown".
  - Elsewhere: `worldmap.msg` 1000 plus the cell's terrain code (SPEC-008 §3.4), giving "Desert", "Mountains", "City" or "Coast".
- The text is drawn in font 101, key 992 [215], with shadow.
- For text width w, the left edge is `x = px - 12 + trunc((25 - w) / 2)`, truncated toward zero, and the top is `y = py - 17`.
- The label is drawn only when `x > 22 - w`, `x < 472` and `11 < y < 463`. It is cut horizontally to x 22-471.
- Only the w-by-10 rectangle is written, so shadow pixels to its right or below it do not appear. Rows above 21 are covered by the frame.

**Clicks in the view.** `[Must-match]` A left-button release with the pointer at x 23-471 and y 22-462 is a view click. Right clicks do nothing, and the sidebar and Town/World events take precedence in the same pass.

- **Party at rest, pointer hovering:** enter here. Play `ib2p1xx1`.
  - In a town cell, enter that town (see "Entering a town" below).
  - Elsewhere, drop to a terrain map: SPEC-008 §3.5 choice, campaign index 65 set, index 32 = 1.
- **Otherwise:** play `ib1p1xx1` and set the destination to `(vx + mx - 22, vy + my - 21)` for pointer (mx, my), with no jitter. Travel starts, or restarts from the current pixel, as in SPEC-008 §3.4. Travel needs no known town or discovered cell. While the party travels, a click near it retargets instead of entering.

**Town buttons.** `[Must-match]` On the event for town t:

- Nothing happens unless t's known/visited bit is set. The button sounds still play.
- Nothing happens while the party is already travelling to a target inside t's cell.
- If the party's cell is not t's cell, the party travels to t's jittered center (SPEC-008 §3.7).
- If the party is in t's cell, the party enters town t. That holds even while it is travelling away inside the cell.

**Arrival.** `[Must-match]`

- When a journey ends inside a town cell, the party stops at rest on the map. The town is never entered automatically, whatever started the journey, and a hotspot chosen on a town map before the journey is not used.
- The player enters by clicking the party marker or the town's button.
- A journey that ends elsewhere also leaves the party at rest.
- When the walking mask stops travel (SPEC-008 §3.4), the party simply stops at its last legal pixel, with no text and no sound.
- Crossing a town cell without stopping discovers nothing beyond the SPEC-008 reveal. Stopping in an undiscovered town's cell shows "Unknown" on hover, and entering it there discovers the town (below).

**Entering a town or dropping to terrain.** `[Must-match]`

1. The view is redrawn without markers and is not recentered.
2. `hotspot2.frm` is drawn at the triangle's position, with the hover label when the pointer hovers, and the window is presented.
3. The wait depends on how entry started. A town-button entry while travelling waits 500 milliseconds. Any other entry waits until the left button is no longer held, then 300 milliseconds.
4. A terrain drop then loads its map. Entering town t continues as follows:
   - **t's known/visited bit is clear:** load the map of t's slot 0 directly (SPEC-008 §3.7, including the destroyed-area substitutions) and set the bit. This is how a town is first discovered.
   - **Otherwise:** t becomes the current town, and the town map of t opens (section 3.40).

**Town-map results.** `[Must-match]` unless tagged. The town map returns a choice, which this screen handles as follows:

- **A hotspot of the town the party is in:** load that hotspot's map (section 3.40).
- **A hotspot of another town:** the party travels to that town's jittered center and the world map stays open. Arrival is as above.
- **The World button:** return to this screen at rest. Any journey in progress is cancelled.
- **A failure** (section 3.40): leave with no map loaded, as in SPEC-013 §3.22.
- **A quit request:** leave without loading.
  - When the town map was opened by entering a town, the reference reads the result's town even though the town map never set it. It then travels toward, or loads a map of, an undefined town before quitting. `[May-improve]`: leave at once, with no draw, travel or load.

**Town/World button** (event 512). `[Must-match]`

- The button opens the town map. It shows the destination town while a journey chosen on a town map is under way, and the current town otherwise.
- A journey in progress stops where the party is.
- The results are handled as above. A hotspot chosen in the town the party stands in is loaded at once.

**Encounters.** When SPEC-008 §3.5 selects an encounter, the following happens. `[Must-match]`

1. The view is centered on the party (clamped) and redrawn without markers.
2. The party's position flashes 7 times, and nothing interrupts the flash:
   - **Ordinary encounter:** `wmapfgt0.frm`, then `wmapfgt1.frm`, at (px - 3, py - 5).
   - **Special encounter:** `wmaptrg2.frm`, then `wmaptarg.frm`, at (px - 5, py - 5).
   - Each image is presented and held for 199 milliseconds, so the 14 images take 2786 milliseconds.
3. The encounter map loads as in SPEC-008 §3.5.

A queue event that stops travel (SPEC-008 §3.4) leaves without the pressed triangle or the flash. In a town cell it enters the town (slot 0 on first discovery, otherwise the town map). Elsewhere it drops to a terrain map with campaign index 32 = 1.

**Campaign checks on every pass.** These checks run after any travel iterations, on every pass, travelling or not. They exist only on this screen. `[Must-match]`

1. If campaign index 10 is 0 and index 101 is not 2:
   - the screen turns black (key 0 [228]) and the buttons are disabled;
   - `boil3` plays (SPEC-012);
   - a quit request is made and the screen leaves.
2. Otherwise, the Military Base vats explode when index 147 is nonzero and the clock is at least 2410 ticks past it:
   - `vexpld` plays and index 308 becomes 2.
   - If index 18 is nonzero, the party is moved to Vault 13 by script-directed travel (SPEC-008 §3.4). `V13ENT.MAP` (map 35) then loads with entry selector 0, and the screen leaves after this pass.
   - The player gains 10000 experience (SPEC-011), and index 155 increases by 5, clamped to -100 through 100.
   - `worldmap.msg` 500 ("You gained 10,000 experience points!") is printed to the message monitor.
   - Index 147 becomes 0 and index 17 becomes 1.
3. Then, under the same timing rule for index 55, the Master's lair explodes:
   - `cathexp` plays and index 309 becomes 2.
   - If index 17 is nonzero, the same travel and load follow.
   - The player gains 10000 experience and index 155 increases by 10, with the same clamp. No message is printed.
   - Index 55 becomes 0 and index 18 becomes 1.

When both countdowns expire in the same pass, step 2 still sees index 18 as 0, and step 3 sees the index 17 that step 2 has just set, so only step 3 moves the party. The tick difference is taken as unsigned 32-bit, so a stored tick later than the clock fires at once. `[May-improve]`: fire only when the clock is at least 2410 ticks past the stored tick.

**Keys.** `[Must-match]`

| Key | Effect |
| --- | --- |
| `A`, `a` | Toggle follow mode |
| `C`, `c` | Center on the party once |
| Arrows, Home, End | Scroll as above |
| `P`, `p`, Ctrl+P, Alt+P | World-map pause window (section 3.17) |
| Ctrl+Q, Ctrl+X, F10 | Quit confirmation (section 3.17). A confirmed quit leaves without loading a map |
| F12 | Screenshot |
| `+`, `=` / `-`, `_` | Brightness up / down, saved immediately (section 3.4) |

No key and no button closes the world map; Esc does nothing. The screen is left only by entering a location, an encounter, a queue event, a campaign check, a town-map result or a quit. `[Must-match]`

**Leaving to a map.** `[Must-match]`

1. The buttons are disabled, and the window is filled with key 0 [228] and presented.
2. Campaign index 32 is set to the entry selector, and the map loads by name, so map numbers are assigned per SPEC-008 §3.10.
3. The city music starts (SPEC-008 §3.10) and the player is shown.
4. The indicator boxes are switched on again if they were on when the screen was last built.
5. The world-map window is removed after the screen returns.

Step 4 tests the state at the last build, and a rebuild finds the boxes already switched off. So once a town map has returned to this screen (World button, or a journey to another town), the next map load leaves the boxes off. A load straight from a town-map choice, without a rebuild, restores them. Only a game load, a new game or closing the character editor brings them back. `[May-improve]`: restore the indicator boxes to their state when the world map first opened.

### 3.40 Town Map Screen

The town map shows one town's picture with a marker for each known entrance. SPEC-008 §3.7 owns the hotspot catalog (anchors, entry selectors, file names), entrance knowledge, the event numbering and the destroyed-area substitutions. This section adds the screen, the per-town details and what a choice leads to.

**Opening.** `[Must-match]` unless tagged.

- The town map opens from:
  - an exit grid holding map -1, or the `world_map` opcode, for the current town (section 3.39);
  - the world map's Town/World button;
  - entering a known/visited town on the world map.
- First the current map is saved into the working map set, with the same failure handling as section 3.39. A pending quit request returns at once.
- The town's knowledge is refreshed as in SPEC-008 §3.7.
- Music is unchanged: the map's city track continues, or `03wrldmp` when the town map was opened from the world map.
- Map background processing and palette cycling are suspended until the town map closes. The cursor is the arrow and the font is 101.
- Town ids above 12 fail. Town id 12 and negative ids are not rejected and use the wrong art. `[May-improve]`: treat any town outside 0-11 as a failure. A failure means art that cannot be loaded or an invalid town; it returns at once and leads to the empty-map case of SPEC-013 §3.22.

**Window and art.** `[Must-match]`

- The window is the 640-by-480 window of section 3.39 at screen (0, 0), created by whichever screen opens first.
- The town picture, list index 156 + t for town t (table below, 453 by 444), is copied opaque to (20, 20).
- `wmapbox.frm` is then drawn over it with transparency. Only picture columns 2-451 and rows 1-442 show, through the same hole at (22, 21), 450 by 442.
- The side panel is the world map's:
  - the town buttons and names, which follow the same known/visited rule;
  - the Town/World button;
  - the date and time.
  The same list indices are used.

| Town | Picture | List index |
| ---: | --- | ---: |
| 0 Vault 13 | `twnmap00.frm` | 156 |
| 1 Vault 15 | `twnmap01.frm` | 157 |
| 2 Shady Sands | `twnmap02.frm` | 158 |
| 3 Junktown | `twnmap03.frm` | 159 |
| 4 Raiders | `twnmap04.frm` | 160 |
| 5 Necropolis | `twnmap05.frm` | 161 |
| 6 Hub | `twnmap06.frm` | 162 |
| 7 Brotherhood | `twnmap07.frm` | 163 |
| 8 Military Base | `twnmap08.frm` | 164 |
| 9 Glow | `twnmap09.frm` | 165 |
| 10 Boneyard | `twnmap10.frm` | 166 |
| 11 Cathedral | `twnmap11.frm` | 167 |

A destroyed Cathedral or Military Base keeps its picture. Only its markers change. `[Must-match]`

**Which markers are shown.** `[Must-match]`

- The shown row of the SPEC-008 §3.7 catalog is the town's own row, with two exceptions: row 12 for the Cathedral when campaign index 18 is nonzero, and row 13 for the Military Base when index 17 is nonzero.
- Row 14 (`BRODEAD.MAP`) is never shown and never loaded from this screen.
- Slots are taken in order until the first zero anchor. Only known slots get a marker, so a town shows between 0 and its number of catalog slots.
- Vault 13 shows only "Cavern Entrance" before tick 864000 and all four markers from that tick on. Its campaign entrance indices are never read.

**Marker geometry.** `[Must-match]` For anchor (X, Y), each marker is a 164-by-25 button at (X - 57, Y - 6). It has three images:

| State | Image |
| --- | --- |
| Normal | `hotspot1.frm` at (69, 12) within the button, that is, at window (X + 12, Y + 6): the hollow triangle |
| Pressed | `hotspot2.frm` at the same place: the filled triangle |
| Pointer over the marker | The hollow triangle, plus the label at the top of the button |

- **Label.** For town t and slot k, the text is `worldmap.msg` 200 + 10t + k. Row 12 uses 290 ("Crater") and row 13 uses 501 ("Ruins"). It is drawn in font 101, key 992 [215], with shadow, at button x `max(0, trunc((164 - w) / 2))` and y 0, and it is cut at 164 pixels. Labels appear only while the pointer is over a marker. Only the picture's own printed captions are always visible.
- **Hit area.** Only the pixels where `hotspot1.frm` or `hotspot2.frm` is not transparent count, which is the filled triangle's shape. The label and the rest of the button rectangle do not react.
- **Sounds.** `ib2p1xx1` plays on press. There is no release sound. The choice is made on release.
- When the pointer leaves a marker, the town picture is restored under the label and the triangle, and the normal triangle is drawn again.

**Hotspot table.** The table is exact data. `[Must-match]`

- Anchor and selector are those of SPEC-008 §3.7.
- Triangle is the window position of the 25-by-13 marker image.
- Label is the `worldmap.msg` id and its English text.
- Known index is the campaign index whose low byte enables the slot, per SPEC-008 §3.7. Slot 0 is also known when the town's known-town value is 1.
- Header entry is the map file's default entry hex and elevation (SPEC-003 §2.7, corpus values).

| Town | Slot | Anchor | Triangle | Label | Map | Selector | Known index | Header entry |
| ---: | ---: | --- | --- | --- | --- | ---: | ---: | --- |
| 0 | 0 | 202, 303 | (214, 309) | 200 "Cavern Entrance" | 35 `V13ENT.MAP` | 0 | time | 20100, 0 |
| 0 | 1 | 271, 282 | (283, 288) | 201 "Entrance" | 6 `VAULT13.MAP` | 1 | time | 17095, 0 |
| 0 | 2 | 292, 237 | (304, 243) | 202 "Living Quarters" | 6 `VAULT13.MAP` | 2 | time | 17095, 0 |
| 0 | 3 | 309, 204 | (321, 210) | 203 "Command Center" | 6 `VAULT13.MAP` | 3 | time | 17095, 0 |
| 1 | 0 | 68, 250 | (80, 256) | 210 "Desert Entrance" | 7 `VAULTENT.MAP` | 0 | 562 | 20100, 0 |
| 1 | 1 | 107, 209 | (119, 215) | 211 "Caverns/Entrance" | 8 `VAULTBUR.MAP` | 1 | 563 | 20100, 0 |
| 1 | 2 | 298, 187 | (310, 193) | 212 "Living Quarters" | 8 `VAULTBUR.MAP` | 2 | 564 | 20100, 0 |
| 1 | 3 | 135, 290 | (147, 296) | 213 "Command Center" | 8 `VAULTBUR.MAP` | 3 | 565 | 20100, 0 |
| 2 | 0 | 158, 192 | (170, 198) | 220 "Entrance" | 26 `SHADYW.MAP` | 1 | 566 | 20100, 0 |
| 2 | 1 | 270, 253 | (282, 259) | 221 "Garden" | 25 `SHADYE.MAP` | 2 | 567 | 20100, 0 |
| 2 | 2 | 314, 217 | (326, 223) | 222 "Brahmin Pen" | 25 `SHADYE.MAP` | 3 | 568 | 20100, 0 |
| 3 | 0 | 400, 317 | (412, 323) | 230 "Entrance" | 10 `JUNKENT.MAP` | 3 | 569 | 27307, 0 |
| 3 | 1 | 304, 257 | (316, 263) | 231 "Crash House" | 12 `JUNKKILL.MAP` | 2 | 570 | 22703, 0 |
| 3 | 2 | 200, 279 | (212, 285) | 232 "Casino" | 11 `JUNKCSNO.MAP` | 1 | 571 | 20100, 0 |
| 4 | 0 | 241, 398 | (253, 404) | 240 "Base" | 24 `RAIDERS.MAP` | 1 | 572 | 20100, 0 |
| 5 | 0 | 398, 265 | (410, 271) | 250 "Hotel" | 4 `HOTEL.MAP` | 3 | 573 | 16325, 1 |
| 5 | 1 | 239, 224 | (251, 230) | 251 "Hall" | 3 `HALLDED.MAP` | 2 | 574 | 20100, 0 |
| 5 | 2 | 79, 207 | (91, 213) | 252 "Watershed" | 5 `WATRSHD.MAP` | 1 | 575 | 20100, 0 |
| 6 | 0 | 238, 78 | (250, 84) | 260 "Entrance" | 36 `HUBENT.MAP` | 1 | 576 | 20100, 0 |
| 6 | 1 | 205, 172 | (217, 178) | 261 "Downtown" | 38 `HUBDWNTN.MAP` | 3 | 577 | 15097, 0 |
| 6 | 2 | 128, 138 | (140, 144) | 262 "Heights" | 39 `HUBHEIGT.MAP` | 5 | 578 | 18693, 0 |
| 6 | 3 | 306, 137 | (318, 143) | 263 "Old Town" | 40 `HUBOLDTN.MAP` | 2 | 579 | 17920, 0 |
| 6 | 4 | 272, 238 | (284, 244) | 264 "Merchants" | 41 `HUBWATER.MAP` | 4 | 580 | 20100, 0 |
| 6 | 5 | 125, 216 | (137, 222) | 265 "Cave" | 37 `DETHCLAW.MAP` | 0 | 581 | 13929, 0 |
| 7 | 0 | 172, 167 | (184, 173) | 270 "Entrance" | 13 `BROHDENT.MAP` | 1 | 582 | 25103, 0 |
| 7 | 1 | 254, 194 | (266, 200) | 271 "Level 1" | 14 `BROHD12.MAP` | 2 | 583 | 20100, 0 |
| 7 | 2 | 136, 263 | (148, 269) | 272 "Level 2" | 14 `BROHD12.MAP` | 3 | 584 | 20100, 0 |
| 7 | 3 | 280, 306 | (292, 312) | 273 "Level 3" | 15 `BROHD34.MAP` | 4 | 585 | 20100, 0 |
| 7 | 4 | 161, 373 | (173, 379) | 274 "Level 4" | 15 `BROHD34.MAP` | 5 | 586 | 20100, 0 |
| 8 | 0 | 197, 83 | (209, 89) | 280 "Entrance" | 30 `MBENT.MAP` | 0 | 587 | 20100, 0 |
| 9 | 0 | 340, 149 | (352, 155) | 290 "Crater" | 27 `GLOWENT.MAP` | 0 | 592 | 16292, 0 |
| 9 | 1 | 334, 195 | (346, 201) | 291 "Level 1" | 42 `GLOW1.MAP` | 1 | 593 | 20100, 0 |
| 10 | 0 | 276, 239 | (288, 245) | 300 "Adytum" | 28 `LAADYTUM.MAP` | 3 | 594 | 20100, 0 |
| 10 | 1 | 229, 195 | (241, 201) | 301 "Downtown" | 44 `LABLADES.MAP` | 4 | 595 | 20100, 0 |
| 10 | 2 | 179, 185 | (191, 191) | 302 "Library" | 29 `LAFOLLWR.MAP` | 5 | 596 | 22882, 0 |
| 10 | 3 | 346, 114 | (358, 120) | 303 "Fortress" | 46 `LAGUNRUN.MAP` | 1 | 597 | 20100, 0 |
| 10 | 4 | 285, 159 | (297, 165) | 304 "Warehouse" | 45 `LARIPPER.MAP` | 2 | 598 | 24503, 0 |
| 11 | 0 | 86, 328 | (98, 334) | 310 "Entrance" | 17 `CHILDRN1.MAP` | 0 | 599 | 20500, 0 |
| 11 | 1 | 229, 313 | (241, 319) | 311 "Interior" | 17 `CHILDRN1.MAP` | 1 | 600 | 20500, 0 |
| 11, destroyed (row 12) | 0 | 235, 301 | (247, 307) | 290 "Crater" | 47 `CHILDEAD.MAP` | 0 | always | 23099, 0 |
| 8, destroyed (row 13) | 0 | 106, 64 | (118, 70) | 501 "Ruins" | 48 `MBDEAD.MAP` | 0 | always | 20100, 0 |

"Time" means the Vault 13 rule above. "Always" means that the alternate slot is known whenever the row is shown. Military Base entrance indices 588-591 and labels 281-284 have no catalog slot and are never shown. Vault 15 has no `V15ENT` or `VAULT15` map. Its surface entrance is `VAULTENT.MAP` (map 7, "Buried Vault Entrance" in the `map.msg` comments), reached by slot 0 with selector 0. Slots 1-3 all load the buried vault `VAULTBUR.MAP` (map 8), with selectors 1, 2 and 3. Both maps have the area name "Vault 15" (`map.msg` 107, 108). `[Must-match]`

**Where a chosen entrance places the player.** `[Must-match]` The choice returns the shown town and the slot. When the party is in that town's world-map cell, the following happens:

1. The town becomes the current town.
2. The slot's file loads by name, with the destroyed-area substitutions of SPEC-008 §3.7. Choosing either Cathedral slot while index 18 is set loads `CHILDEAD.MAP` with selector 0. Choosing `MBENT.MAP` while index 17 is set loads `MBDEAD.MAP` with selector 0.
3. Campaign index 32 is set to the selector, and the player is placed at the map's header entry (or at the saved position of a working copy, section 3.35).
4. The map's own script then moves the player according to index 32.

The engine itself derives no tile and no elevation from the selector. Most header entries are the placeholder hex 20100 on elevation 0, so the script's placement is what the player sees. When the party is elsewhere, the world map takes over and the party travels to that town (section 3.39).

**Side panel on the town map.** `[Must-match]`

- A known/visited town's button switches the screen to that town's picture and markers. It does not travel, and it does not change the current town.
- Buttons of other towns do nothing.
- The Town/World button returns "World map", and the world map follows at rest (section 3.39).

**Keys.** `[Must-match]` Ctrl+Q, Ctrl+X and F10 open the quit confirmation; a confirmed quit closes the town map with a quit result. F12 takes a screenshot. `+`, `=`, `-` and `_` change brightness. There is no Esc, no close button and no pause key. The screen is left only by a marker, the Town/World button or a quit.

**Closing.** `[Must-match]`

1. The markers are removed.
2. The picture, the frame, the town names and the date are drawn again, together with the normal triangle of every known marker (no labels), and the window is presented.
3. This image stays until the next screen draws over it: the world map, or the black fill of a map load.
4. Background processing is resumed if the town map suspended it, and palette cycling is resumed.

### 3.41 Pip-Boy Screen

The Pip-Boy 2000 is a full-screen modal window with four pages: Status (quests and holodisks), Automaps, Video Archives and Alarm Clock (rest). This section owns its art, layout, text, input and sounds. The game rules stay where they are: quest and holodisk state in SPEC-008 §3.8, rest eligibility, time advance and healing in SPEC-008 §3.9, the automap raster in section 3.33 and SPEC-008 §2.8, and archive movie selection and playback in SPEC-012 §3.12. R1 has no Clues page. The art shows a scratched-out second label with no button behind it. `[Must-match]`

**Art.** All art is under `art/intrface/`, one frame each. Sizes are from the corpus FRM headers. There is no `pipboy.frm`. `[Must-match]`

| Use | Art (list index) | Size |
| --- | --- | --- |
| Background, the whole window | `pip.frm` (127) | 640 by 480 |
| Page buttons | `lilredup.frm` (8) up, `lilreddn.frm` (9) down: the Red button | 15 by 16 |
| Date and time digits | `numbers.frm` (82), white band: digit d is the 9-by-17 cell at source x = 9d | 360 by 17 |
| Vault-water note | `pip2.frm` (128) | 148 by 227 |
| Month names | `months.frm` (129): month m (1-12) uses source rows 15(m - 1) through 15(m - 1) + 13 | 29 by 179 |
| Note digits | `notenums.frm` (130): digit d is the 12-by-13 cell at source x = 12d | 120 by 13 |
| Alarm-clock button | `alarmout.frm` (132) up, `alarmin.frm` (131) down | 28 by 24 |
| Start-screen logo | `pipx.frm` (133) | 204 by 141 |
| Screen-saver bomb | `bomb1.frm` (226) | 65 by 65 |

The interface bar's `pipup.frm` and `pipdn.frm` (41 by 19) are only the bar's Pip-Boy button (section 3.19). The Pip-Boy window does not use them.

#### 3.41.1 Opening and Closing

**Ways in.** `[Must-match]`

- **`p` or `P`** in the game loop, while the interface bar is enabled. The bar's Pip-Boy button sends `p` (section 3.19). The game loop plays `ib1p1xx1` and opens the Pip-Boy on its start screen.
- **`z` or `Z`**, under the same condition. It plays `ib1p1xx1` and opens the Pip-Boy with the rest intent, which shows the Alarm Clock page at once (see the opening steps below).
- **In combat** both keys are refused. The game plays `iisxxxx1` and shows `misc.msg` 7 "Pipboy not available in combat!" in a dialog box at screen (192, 116) (section 3.13), and the Pip-Boy does not open.
- Nothing else opens the Pip-Boy: no script opcode, no world-map control, and no other window.

**Opening steps**, in order. `[Must-match]` unless tagged.

1. Map background processing is paused if it is running, with the same coverage as in section 3.28: object animation, idle fidgets, critter scripts, timed events, the game clock, floating text, the game mouse and edge scrolling. Palette cycling is suspended. The in-world cursor is turned off and the arrow cursor (section 3.26) is set. Font 101 is selected, and the previous font is remembered.
2. The credited-rest-minute counter of SPEC-008 §3.9 is reset to 0. No page is current yet, and no row zones exist (section 3.41.2).
3. `pipboy.msg` is loaded from `text/<language>/game/`, then the eleven art files above. If any of them fails, the Pip-Boy does not open and gives no message. The reference then leaves step 1 in force: background processing stays paused, cycling stays off and font 101 stays selected. `[May-improve]`: undo step 1 when opening fails.
4. A modal window is created at screen (0, 0), 640 by 480. It covers the map and the interface bar. `pip.frm` is copied into it opaquely. Its art already contains the frame, the five button plates with their labels STATUS, a scratched-out second label, AUTOMAPS, ARCHIVES and CLOSE, and the empty green screen.
5. The vault-water note, the time and the date are drawn (below).
6. The buttons are created (below).
7. The start screen is drawn, except when the rest intent is used and resting is allowed here:
   - `pipx.frm` is drawn transparently at (323, 156).
   - If the game date (SPEC-005 §3.5 calendar) is one of the holidays below, its `pipboy.msg` text is drawn in font 101, key 992 [215], with no background. It goes at y = 315 and x = `431 - trunc(w / 2)`, w being the text width, which centers it on x = 431, the logo center plus 6.

   | Date | ID | Text |
   | --- | ---: | --- |
   | 1 January | 100 | Happy New Year's Day! |
   | 14 February | 101 | Happy Valentine's Day! |
   | 1 April | 102 | Happy All Fool's Day! |
   | 4 July | 104 | Happy Independence Day! |
   | 6 October | 103 | Happy Ship Day! |
   | 31 October | 105 | Happy Halloween! |
   | 28 November | 106 | Happy Thanksgiving Day! |
   | 25 December | 107 | Merry Christmas! |

8. **Rest intent, resting refused** (the SPEC-008 §3.9 eligibility test fails): the start screen is drawn as in step 7 and shown. Then `iisxxxx1` plays and `pipboy.msg` 215 "You can not rest at this location!" appears in a large dialog box at screen (192, 135). Its title is drawn in key 32328 [145] at box x = `trunc((302 - 209) / 2) = 46`, y = 52 (section 3.13). After Done, the Pip-Boy stays open on the start screen with no current page.
9. The sound `pipon` is requested. The corpus has no `pipon.acm`, so nothing is heard (SPEC-010 §3.9).
10. **Rest intent, resting allowed:** in the first loop pass, the polled input is discarded and the pass acts as a press of the Alarm Clock button. The Alarm Clock page then appears without the start screen.

There is no palette fade on opening or closing (section 3.2), and no animation other than the screen saver (section 3.41.8).

**Closing.** Esc, Enter, `p`, `P`, the Close button, a pending quit request, or an Alarm Clock rest that a timed event interrupts (SPEC-008 §3.9) closes the window. Closing then runs these steps in order. `[Must-match]`

1. The map-update scripts run (`map_update_p_proc`, SPEC-006), on every close, whether or not time passed.
2. The window and all its buttons are removed, and `pipboy.msg` is unloaded.
3. The remembered font is restored.
4. Map background processing resumes only if step 1 of opening paused it. Palette cycling resumes.
5. The arrow cursor is set and the interface bar is redrawn.

No closing sound plays.

**Window controls.** Coordinates are window coordinates, which equal screen coordinates here. Each control acts on mouse release inside its rectangle. `[Must-match]`

| Control | Rectangle | Art | Sounds | Sends |
| --- | --- | --- | --- | --- |
| Status | (53, 341) 15 by 16 | Red button, drawn transparently | `ib1p1xx1` press, `ib1lu1x1` release | Status page |
| (none) | (53, 368) | No button: the scratched-out label | | |
| Automaps | (53, 395) 15 by 16 | Red button | Red-button sounds | Automaps page |
| Archives | (53, 422) 15 by 16 | Red button | Red-button sounds | Video Archives page |
| Close | (53, 449) 15 by 16 | Red button | Red-button sounds | Close |
| Alarm clock | (124, 13) 28 by 24 | `alarmout.frm` up, `alarmin.frm` down, drawn transparently | `ib2p1xx1` press, `ib2lu1x1` release | Alarm Clock page |

The whole rectangle of each button is clickable; no mask is used. The page buttons, labels included, are part of `pip.frm`. A page button pressed while its page is already shown redraws that page from its initial state. `[Must-match]`

**Keys.** `[Must-match]`

| Key | Action |
| --- | --- |
| Esc, Enter, `p`, `P` | Close |
| Ctrl+Q, Ctrl+X, F10 | Quit confirmation (section 3.17); the Pip-Boy then closes whatever the answer |
| F12 | Screenshot |
| PageDown, PageUp | Sent to the current page as "next" and "previous". Only a holodisk reading page uses them |

Every other key does nothing. That includes the arrow keys, Home, End, the digit keys, Tab, F1 and the in-game hotkeys of section 3.19.

**Date and time.** Digits are copied opaquely from the white band of `numbers.frm`, 9 pixels apart, with leading zeros. They are drawn at opening and after every rest step. The display is not otherwise refreshed, since the game clock is paused while the window is open. `[Must-match]`

| Item | Position | Format |
| --- | --- | --- |
| Day of month | (20, 17), 2 digits | `05` |
| Month | (46, 18): a 29-by-14 block from `months.frm`, copied opaquely | JAN through DEC art |
| Year | (83, 17), 4 digits | `2161` |
| Time | (155, 17), 4 digits, 24-hour clock, no separator | `0721` |

**Vault-water note.** The note shows unless campaign global 101 (the water chip) equals 2 or campaign global 10 (the vault-water days, counted down at each midnight as in SPEC-012 §3.12) equals 0. Otherwise the 148-by-227 rectangle at (32, 83) is restored from `pip.frm`. A shown note is `pip2.frm` copied opaquely at (32, 83), with the days left in `notenums.frm` digits drawn transparently over it. The digits are drawn from the ones digit up: the ones digit at (92, 180), and each next digit 12 pixels further left and 2 pixels lower. After a digit 1, the next digit moves only 6 pixels left. The note is redrawn at opening and after every rest step. `[Must-match]` A negative day count shows the note and draws cells from outside the digit art. `[May-improve]`: hide the note when the day count is not positive.

#### 3.41.2 The Screen Area and Row Zones

**Screen area.** The page text area is the rectangle (254, 46) 374 by 410, that is x 254-627 and y 46-455. Drawing a page first restores this rectangle from `pip.frm`. The start-screen logo and any earlier page vanish with it. Pixels outside the rectangle are never restored by a page. `[Must-match]`

**Lines.** Text is in font 101, with a line height of 10 (SPEC-002 §2.4). Line n, for n from 0 to 40, has its text origin at y = `46 + 10 n`. Line 40, at y = 446, is the bottom line. Printing a line moves the text cursor to the next line, but never past line 40. A page's layout often skips a line after each entry, so lists use every other line. `[Must-match]`

**Horizontal placement.** w is the text width (SPEC-002 §3.1). `[Must-match]`

| Mode | Text starts at x |
| --- | --- |
| Normal | 262 |
| No indent (holodisk text) | 255 |
| Centered | `254 + trunc((350 - w) / 2)`, centered on x = 429 |
| Right column | 437 |
| Left-header alignment | `364 - w`, so the text ends at x = 364 |
| Right-header alignment | `522 - w`, so the text ends at x = 522 |

Text is drawn over the screen with no background fill. Its drawing limit is 640 pixels, so a long line is not clipped at the screen edge. Underlined text uses the font underline of SPEC-002 §3.1, row 8 below the text origin. A struck-through line has a 1-pixel line at text origin y + 3, from the text's left x across its full advance width w, in the text color. `[Must-match]`

**Colors.** `[Must-match]`

- Ordinary text: key 992 [215].
- The selected or pressed row: key 32747 [57], selection yellow.
- Completed quests: key 8804 [218], struck through.
- No hover color exists. Nothing changes when the pointer moves over a row.

**Row zones.** Clickable rows are invisible, art-less rectangles 350 by 10 at x = 254, one line tall. They make no sound of their own; the page plays `ib1p1xx1` when it accepts a click. A zone created for line n covers y = `46 + 10 n` through `+ 9`. A page creates its zones starting at a first line s, one per entry, on lines s, s + 2, s + 4 and so on. Zone k, counted from 0, reports row number `s + k - 1`. A zone is created only while `s + k` is below 20, so at most 18 zones exist for s = 2. A page may also create a **Back zone** on line 40, (254, 446) 350 by 10. `[Must-match]`

A click's horizontal position, where a page tests it, is the pointer x when the release is processed. A page that accepts a click usually redraws the clicked entry in yellow, shows it for 200 milliseconds, then acts. `[Must-match]`

Zone defects in R1:

- **Back zones stay.** A Back zone is never removed when its page is left. It stays until the window closes, and every new one stacks over it. On the Status list (section 3.41.3), a release on line 40 left of x = 429 then plays `ib1p1xx1` twice, flashes a yellow BACK there for 200 milliseconds, and redraws the list. On the other list pages it does nothing. `[May-improve]`: remove the Back zone when leaving the page that created it.
- **Removal overrun.** Removing a page's zones visits as many entries as the page asked for, even beyond the 18 that exist. When quest locations plus holodisks reach 18 or more, the Status page asks for more than 18, and the removal goes past its table. `[May-improve]`: remove only the zones that were created.

**Text cursor overflow.** Lines printed after line 40 overprint line 40. No shipped content reaches it. `[Must-match]`

#### 3.41.3 Status Page

Quest visibility, the water-chip rule, holodisk acquisition, and the 35-record page budget are SPEC-008 §3.8's. This section places them. `[Must-match]` unless tagged.

**List view**, drawn by the Status button:

- Line 0: `pipboy.msg` 202 "STATUS", underlined. When any holodisk global is nonzero, it is right-aligned to end at x = 364, and 211 "DATA", underlined, is right-aligned to end at x = 522 on the same line. Otherwise STATUS is centered (x = 405).
- From line 2, every other line: one location title per visible quest group (`pipboy.msg` 700 + 10 × town), at x = 262. The water-chip quest keeps Vault 13 always listed, so the alternative text 203 "No current tasks." can never show in R1.
- From line 2, every other line, right column x = 437: the titles of acquired holodisks (`pipboy.msg` 400 + ordinal), in ordinal order.
- Zones start at line 2. The page asks for one zone per quest location plus one per holodisk plus one more, and at most 18 are created. Row number r (from 1) is the r-th location on the left and the r-th holodisk on the right.

**Clicks on the list.**

- **Left, x < 429**, row r: `ib1p1xx1`, then location r in yellow for 200 milliseconds, then its detail view. A left click on a row past the last location still plays the sound and the 200-millisecond pause, but leaves the list in a state where only location rows respond, by opening their detail views directly. Holodisk rows stop working until the Status button is pressed again. PageUp and PageDown with the pointer left of x = 429 do the same. `[May-improve]`: ignore left clicks below the last location and page keys on the list.
- **Right, x > 429**, row r no greater than the holodisk count: `ib1p1xx1`, then holodisk mapping (SPEC-008 §3.8). Then one title in yellow for 200 milliseconds, and the reading view. The highlighted title is the row whose position equals the chosen disk's ordinal, not the clicked row. Unless every lower-ordinal disk is also held, the wrong title or none lights. `[May-improve]`: highlight the clicked row.
- x = 429 exactly does nothing.

**Detail view** of a quest location:

- Line 1: location title, a space, and `pipboy.msg` 210 "Status", underlined, at x = 262, for example "Junktown Status".
- From line 3: each visible quest of the group, numbered `1.`, `2.` and so on, compactly, as "N. text" from `pipboy.msg` 701 + 10 × town + position. Each is wrapped to 350 pixels by the rule below. Every wrapped line is followed by one blank line. An active quest uses key 992; a completed quest uses key 8804 [218], struck through.
- Line 40: 201 "BACK", centered (x = 413), and a Back zone.
- Back: `ib1p1xx1`, BACK in yellow for 200 milliseconds, then the list view. Page keys do nothing here.

**Wrapping.** This is the shared word wrapper, here at width 350. `[Must-match]`

1. If the string's width is below 350, it is one line.
2. Otherwise, scan character by character, summing each character's advance: its width plus the letter spacing, with a space taking the font's space width (SPEC-002 §3.1).
3. While the sum is at most 350, remember the last space-class character (space, tab, line feed, vertical tab, form feed, carriage return) or hyphen.
4. When the sum exceeds 350, end the line just after the remembered character, which stays at the end of the earlier line, and rescan from the character after it with the sum at 0. With nothing remembered, the line ends before the current character, which starts the next line.
5. At most 63 lines are made. A wrap failure skips that quest's text, though its number is still used.

No shipped English quest needs a second line: the widest, "9. " plus `pipboy.msg` 751, is 301 pixels.

**Reading view** of holodisk ordinal o, with page p counted from 0:

- Line 0, on page 0 only: the title (`pipboy.msg` 400 + o), centered and underlined.
- When the disk has more than one page, a counter is drawn at y = 48: the page number p + 1, a space, `pipboy.msg` 212 "of", a space, and the page count (last-page index + 1). Its x is 580 minus the width of "of", which is 567 in font 101. It appears on every page. Only the width of "of" is subtracted, so the counter's left edge is fixed and longer counters grow to the right. `[Must-match]`
- From line 3: up to 35 records in no-indent mode (x = 255), one record per line with no wrapping. `**END-PAR**` leaves a blank line. The widest shipped English line is 359 pixels wide and runs to x = 613.
- Line 40: 201 "BACK" right-aligned to end at x = 364, and 200 "MORE" or, on the last page, 214 "DONE", right-aligned to end at x = 522. A Back zone covers the line.
- **Zone and keys:**

  | Input | Not last page | Last page |
  | --- | --- | --- |
  | Release on line 40 with x ≥ 460, or PageDown | Next page | Release: list view; PageDown: nothing |
  | Release on line 40 with x ≤ 395 | Previous page; list view from page 0 | Same |
  | Release on line 40 with x 396-459 | Nothing | Nothing |
  | PageUp | Previous page; list view from page 0 | Same |

  Every accepted action plays `ib1p1xx1` and redraws BACK and MORE or DONE, both in key 992, so the press shows no color change. It then waits 200 milliseconds before acting.

#### 3.41.4 Automaps Page

The page lists areas from the automap database (SPEC-008 §2.8), then the captured elevations of the chosen area, then shows the raster of section 3.33. The database is read again each time a list is drawn. If it cannot be read, only the title is shown. `[Must-match]` unless tagged.

**Area list.**

- Line 0: `pipboy.msg` 205 "AUTOMAPS", centered and underlined.
- From line 2, every other line, at x = 262: one entry per **area group**. Maps are scanned from 0 to 65. A map with a positive offset on at least one elevation joins the list unless an already-listed map appears in its related-map row (table below). Each group is therefore listed under its lowest-numbered captured map.
- The entry text is that map's short name (`map.msg`, section 3.14 table). Entries are sorted by byte-wise text comparison, ascending. The shipped groups have distinct names.
- Zones start at line 2, one per entry. Row r (from 1) selects entry r: `ib1p1xx1`, then the elevation view.
- Before the elevation view replaces it, the list is drawn once with the "selected" entry in yellow. That entry is chosen as the one on line r + 1. It is right only for row 1, and the list is replaced in the same instant. `[May-improve]`: highlight the clicked row, or skip this redraw.

**Related-map table.** This is a fixed table of five positions per map number, in position order. Maps not listed have five empty positions. The relation is used only by this page. `[Must-match]`

| Maps | Row entries, by position |
| --- | --- |
| 3 / 4 / 5 / 9 | 9, 4, 5 / 3, 9, 5 / 3, 4, 9 / 3, 4, 5 |
| 6 / 35 | 35 / 6 |
| 7 / 8 | 8 / 7 |
| 10 / 11 / 12 | 11, 12 / 10, 12 / 10, 11 |
| 13 / 14 / 15 / 55 | 14, 15, 55 / 13, 15, 55 / 13, 14, 55 / 13, 14, 15 |
| 16 / 25 / 26 | 25, 26 / 16, 26 / 16, 25 |
| 17 / 18 / 33 / 34 / 47 | 18, 47, 33, 34 / 17, 47, 33, 34 / 34, 47, 17, 18 / 33, 47, 17, 18 / 17, 18, 33, 34 |
| 27 / 42 / 43 | 42, 43 / 27, 43 / 27, 42 |
| 28 / 29 / 44 / 45 / 46 | 29, 44, 45, 46 / 28, 44, 45, 46 / 28, 29, 45, 46 / 28, 29, 44, 46 / 28, 29, 44, 45 |
| 30 / 31 / 32 / 48 | 31, 32, 48 / 30, 32, 48 / 30, 31, 48 / 30, 31, 32 |
| 36 through 41 | The other five of 36-41, ascending |

**Elevation view** of the chosen map A:

- Line 0: AUTOMAPS, centered and underlined.
- Line 2: `map.msg` 100 + A (the map's area description), centered.
- From line 4, every other line, at x = 262: first each elevation e of A, from 0 to 2, with a positive offset. Its text is `map.msg` 200 + 3A + e, which is often empty, but the row still counts.
- Then R1 visits positions i = 0 through 4 of A's related-map row. For each position i that holds a map, skipping i = A, it lists the captured elevations of **map number i** itself, not of the map stored there, with map i's elevation names. Two results follow:
  - The other maps of a group never appear.
  - Necropolis maps 3 (`HALLDED`) and 4 (`HOTEL`) appear under areas whose rows have entries at those positions: the Hub group for both, and the Cathedral and Boneyard groups for map 3, when those maps have captures.

  `[May-improve]`: for each position holding a map M other than A, list M's captured elevations under M's names.
- Line 40: BACK, centered, with a Back zone.
- Zones start at line 0, covering the entries plus two: line 0 reports row -1, line 2 row 0, and line 4 + 2(r - 1) row r.
- The first entry is shown in yellow, and its raster is drawn at once (section 3.33) over the text, from window (238, 105).

**Clicks in the elevation view.**

- Row r (from 1): `ib1p1xx1`. The view is redrawn with entry r in yellow and its raster drawn.
- Back, or the title line (row -1): the area list is redrawn, then `ib1p1xx1` plays.
- The description line (row 0) does nothing.
- Page keys do nothing.

Raster cells that fall outside the screen area, at x 238-253, x 628-637, and y beyond 455, are drawn over the frame art. No later page erases them until the window closes. `[May-improve]`: restore those pixels from `pip.frm` whenever the screen area is redrawn.

#### 3.41.5 Video Archives Page

`[Must-match]`

- Line 0: `pipboy.msg` 206 "VIDEO ARCHIVES", centered and underlined.
- From line 2, every other line, at x = 262: one entry per movie index 3 through 13 whose played byte is set (SPEC-012), in index order. The text is `pipboy.msg` 500 + index:
  - 503 Vat Destruction; 504 Cathedral Destruction; 505 Overseer Briefing; 506 Vault 13 Water Deprivation; 507 Vault 13 Invasion;
  - 508 and 509 Leaving Vault; 510 Dipped in the Vats; 511 Vault 13 Water; 512 Vault 13 Water Running Low; 513 Rae Kills the Master.
- IDs 500-502 (logos and intro) are never listed. Index 13 never is either, because its movie is absent from the corpus and is never marked played (SPEC-012). Overseer Briefing is listed in every game, since the new-game movie marks it.
- Zones start at line 2, one per entry. Row r: `ib1p1xx1`, then entry r in yellow. The movie chosen by the SPEC-012 §3.12 Pip-Boy selection plays with flags `0x0B`: fade out first, fade the game palette back in after, pause music.
- Afterwards font 101 is selected again, the idle timer of section 3.41.8 restarts, and the list is redrawn without a highlight.
- Page keys do nothing.

#### 3.41.6 Alarm Clock Page

The Alarm Clock button, or the rest intent, first runs the rest eligibility test of SPEC-008 §3.9. `[Must-match]` unless tagged.

**Refused.** `iisxxxx1` plays, and `pipboy.msg` 215 "You can not rest at this location!" is shown in the large dialog box at screen (192, 135), as in section 3.41.1, step 8. The screen area is left as it was. However, the Alarm Clock becomes the current page while the previous page's row zones remain. A later click on such a zone, at row 4 through 16, starts the rest option for that row (row 4 is ten minutes) with no eligibility test. `[May-improve]`: after a refusal, keep the previous page current, so its zones keep their own meaning.

**Allowed: the page.**

- Line 0: `pipboy.msg` 300 "ALARM CLOCK", centered and underlined.
- At y = 66, the strip (254, 66) 350 by 10 is restored from `pip.frm`. Then "Hit Points cur/max" (301 "Hit Points", a space, current HP, `/`, maximum HP), centered in key 992, for example "Hit Points 25/30" at x = 377.
- From line 5, every other line (y = 96 + 20j for j = 0 to 12), at x = 262: options j, with text 302 + j, as below. A zone covers each option line, and option j reports row j + 4.

| j | ID | Text | Rest (SPEC-008 §3.9) |
| ---: | ---: | --- | --- |
| 0 | 302 | Rest for ten minutes | 0 h 10 min |
| 1 | 303 | Rest for thirty minutes | 0 h 30 min |
| 2-7 | 304-309 | Rest for an hour; two; three; four; five; six hours | j - 1 hours |
| 8 | 310 | Rest until morning (0600) | Until 06:00 |
| 9 | 311 | Rest until noon (1200) | Until 12:00 |
| 10 | 312 | Rest until evening (1800) | Until 18:00 |
| 11 | 313 | Rest until midnight (0000) | Until 00:00 |
| 12 | 314 | Rest until healed | Until healed |

An "until" option with wake hour W, at current time h:m, uses d = (W - h) mod 24, in 0 through 23. With m = 0 it rests d hours, or 24 hours when d = 0. With m > 0 it rests (d - 1) mod 24 hours and 60 - m minutes. Either way it ends at the next W:00 strictly after now. No message lists an "until party healed" option.

**Choosing an option.**

1. `ib1p1xx1` plays and the option is redrawn in yellow.
2. The animated watch cursor `wait2.frm` (section 3.26) is shown.
3. The rest runs in steps of at least 50 milliseconds each (SPEC-008 §3.9). After each step, the time digits, the date, the note, and (in the hour phase) the Hit Points line are redrawn and shown. The Hit Points line is also redrawn after the minute phase. There are no clock hands: the digits change.
4. Input is polled once per step. Esc stops the rest and the Pip-Boy stays open. Every other key and click polled during the rest is discarded. A pending quit request also stops it.
5. At the end, the time, date and note are drawn once more, and the arrow cursor returns.
6. `ib2lu1x1` plays and the page is redrawn with no option highlighted.
7. If a timed event ended the rest, the Pip-Boy then closes. If a quit request did, it closes on the next loop pass.

"Rest until healed" with full HP passes no time, though steps 1, 6 and the redraw still happen.

Long rests do not restart the idle timer of section 3.41.8. When a rest took more than 120 seconds of real time, the screen saver can start right after it.

Page keys do nothing on this page.

#### 3.41.7 Start-of-Session State

R1 keeps the current page and its private view state between openings of the Pip-Boy. Opening shows no page, but PageUp and PageDown are still sent to the page current at the last close. After closing on a holodisk reading view, PageDown at the next opening draws that disk's next page. PageUp draws its previous page, or the Status list. After closing on the Status list, a page key with the pointer left of x = 429 draws the remembered Status list over the start screen. `[May-improve]`: ignore page keys until a page button has been pressed in the current opening.

#### 3.41.8 Screen Saver

`[Must-match]` unless tagged.

**Start.** Each main-loop pass that polls no input and finds the pointer where it was compares the time against the idle timer. The timer restarts at opening, on any input or pointer movement, after an archive movie, and when the screen saver ends. Once more than 120000 milliseconds have passed, the screen saver runs. The Alarm Clock rest steps do not restart the timer.

**Run.**

1. The screen area (section 3.41.2) is saved and restored to the bare `pip.frm` screen. Sixteen bomb slots start inactive, and a warm-up counter starts at 50.
2. Each pass first checks for input and pointer movement. Any input or movement from the starting pointer position ends the screen saver, and that input is consumed.
3. One draw in the range 0 through 32767 is made from the game random generator (SPEC-006 §3.4). If it is below 3047.3311, so 0 through 3047, and a slot is free, the lowest free slot gets a new bomb, with two more draws in this order:
   - **Position.** Let v be the second draw and P = `trunc(v / 32767 × 724)` in double precision, where 724 = `(350 - 16) + (406 - 16)`. If P ≤ 394, the bomb starts at (602, P + 48) on the right edge. Otherwise it starts at (P - 124, -17), above the top.
   - **Speed.** Let s be the third draw. The speed is `s × 2.75 / 32767 + 0.15`, stored in single precision, with progress 0.
   
   With no free slot, only the first draw is made.
4. While the warm-up counter is above 0, nothing is drawn and there is no wait; the counter drops by 1 at the end of the pass. Once it reaches 0, each pass restores the screen area from `pip.frm`, draws the bombs, shows the screen area, and waits until 50 milliseconds have passed since the pass began.
5. Each active bomb is clipped against x 254-603 and y 46-451, then drawn transparently from `bomb1.frm`, in slot order:
   - **Horizontal.** If x ≥ 254, the width is 65, or `604 - x` when `x + 65 ≥ 604`; a width below 1 frees the slot. If x < 254, `254 - x` columns are skipped (65 or more frees the slot), and the rest is drawn from x = 254.
   - **Vertical.** If y ≥ 46, the height is 65, or `452 - y` when `y + 65 ≥ 452`; a height below 1 frees the slot. If y < 46, the slot is freed when `y + 65 < 46`; otherwise `46 - y` rows are skipped and the rest is drawn from y = 46.
6. Then every bomb that was active at the start of step 5 gains its speed in progress. When the progress reaches 1.0, the bomb moves to x = `trunc(x - progress)` and y = `trunc(y + progress)`, computed in single precision, and the progress returns to 0. Bombs drift down and to the left.

**End.** The saved screen area is put back and shown. The reference also turns the game mouse back on, which background processing had turned off. For the rest of this opening, the cursor at a screen edge becomes a scroll arrow and the hidden map behind the Pip-Boy edge-scrolls (section 3.29). `[May-improve]`: leave the game mouse off until the Pip-Boy closes.

#### 3.41.9 Message IDs

All `pipboy.msg` IDs used, with English corpus texts. `[Must-match]`

| IDs | Use |
| --- | --- |
| 100-107 | Holiday greetings (section 3.41.1) |
| 200 MORE, 201 BACK, 214 DONE | Paging and Back controls |
| 202 STATUS, 211 DATA, 210 Status, 212 of | Status headers, detail header suffix, page counter |
| 203 No current tasks. | Status fallback, unreachable in R1 |
| 205 AUTOMAPS, 206 VIDEO ARCHIVES, 300 ALARM CLOCK | Page titles |
| 215 You can not rest at this location! | Rest refusal |
| 301 Hit Points | Alarm Clock HP line |
| 302-314 | Rest options |
| 400-417 | Holodisk titles; 1000 × (ordinal + 1) onward holds their text (SPEC-008 §3.8) |
| 503-513 | Archive titles; 500-502 exist but are never listed |
| 700-810 | Quest location titles (700 + 10 × town) and quests (SPEC-008 §3.8) |

IDs 204, 207-209, 213 and 600-608 exist in the corpus but are not used by R1. They include CLUES, "Page", the button names, and "Welcome to the Rob Co Pipboy 2000.".

### 3.42 Character Screen Outside Creation Mode

This section covers the character editor when it is opened during play. Only the differences from creation mode (sections 3.8-3.11) and the parts that exist only in game are listed. Everything not mentioned here is as in section 3.8. Progression rules (skill points per level, perk timing, eligibility, the perk table) are in SPEC-011 §3.20-§3.22. `[Must-match]` unless tagged.

**Opening.** The screen opens from the `c` key or the Character button of the interface bar (section 3.19), only while the bar is enabled. That includes the player's turn in combat. The key plays `ib1p1xx1`; a button click plays the button's own pair first. Map background processing is suspended while the screen is open.

- A level-up never opens the screen. It sets the pending level-up flag, which shows the LEVEL indicator box (section 3.24).
- Every exit, Done or Cancel, clears that flag, so the LEVEL box goes away even after Cancel.
- There is no palette fade (section 3.2). Palette cycling is off, the cursor is the arrow, and the indicator-box window is hidden while the screen is open.
- On exit the interface bar is redrawn and the HP counter is updated without its drum roll (section 3.21).

**Open sequence.**

1. Take the snapshot used by Cancel. It holds:
   - base and bonus stats and invested skill points;
   - current HP and the name;
   - perk ranks and unspent skill points;
   - tags and traits;
   - the last reconciled level and the pending-perk flag;
   - every skill's current value, used as the decrement floor below.
2. Draw the whole screen.
3. Reconcile skill points and perk picks for the levels gained since the last visit (SPEC-011 §3.21).
4. If a perk pick is pending, switch to the Perks folder and open the perk chooser (below) over the screen.
5. Redraw the skills, SPECIAL rows, derived stats and info card, then run the input loop.

**Remembered state.** These persist from one opening to the next:

- the selected info line, and with it the card;
- the selected folder;
- the skill cursor.

A new game or a reset sets them to the Strength card, the Perks folder and the first skill. Leaving creation mode resets the info line only. Erase and Load in creation mode reset all three.

**Window and background.** The window is screen (0, 0) 640 by 480. Its background is `edtredt.frm` (list index 177, 640 by 480, one frame), copied opaquely. The creation-mode background `edtrcrte.frm` (list index 169) is used only in creation mode.

**What is drawn differently.**

| Element | In game |
| --- | --- |
| Name, Age, Sex | The three "off" images with their captions (section 3.8), drawn transparently at (9, 0), (154, 0) and (235, 0). They are not buttons |
| SPECIAL number | Big number at (58, Y): the effective value E (SPEC-006 §3.1, 1-10), always the white set, never animated. There are no plus/minus buttons |
| SPECIAL word | `stat.msg` 300 + E: "V. Bad", "Bad", "Poor", "Fair", "Average", "Good", "V. Good", "Great", "Excellent", "Heroic". Font 101 at (103, Y + 8), key 992, or 32747 when selected. Creation mode uses `editor.msg` 200-209 instead |
| CHAR POINTS caption and counter | Replaced by the level block (below) |
| OPTIONAL TRAITS caption, trait list, trait buttons, tag buttons | Absent. The folder tabs and folder lists use the lower-left area |
| TAG SKILLS caption and counter | "SKILL POINTS" (`editor.msg` 112), font 103, key 18979, at (400, 233). The big number at (522, 228) shows the unspent skill points: two digits, "99" for values above 99, not animated on redraw |
| Skill slider | Present (below) |
| OPTIONS button | The same Red button at (343, 454), captioned PRINT (`editor.msg` 103) at (363, 454) in font 103, key 18979. It opens the print file dialog directly (section 3.43) |
| Hit Points condition line | "Hit Points cur/max" shows the current HP; creation mode shows max/max |

DONE and CANCEL are as in section 3.8.

**Level block.** Before each draw the strip (32, 280) 124 by 32 is restored from the background. Font 101, x = 32. Each line is key 992, or 32747 while its info line is selected.

| y | Info line | Text |
| ---: | ---: | --- |
| 280 | 7 | "Level:" (`editor.msg` 113), a space, the level in decimal |
| 291 | 8 | "Exp:" (114), a space, the experience grouped |
| 302 | 9 | "Next Level:" (115), a space, the next threshold grouped (SPEC-011 §3.20), or `------` at level 21 |

**Grouped numbers.** A grouped number is the decimal value without leading zeros. A comma follows the millions digit and the thousands digit whenever those digits are printed: 0 gives "0", 1000 gives "1,000", 15500 gives "15,500", and 1234567 gives "1,234,567". A negative value gives an empty string. The same rule formats experience and quantities in the printed sheet (section 3.43).

**Skill slider.** The skill list is as in section 3.8. One row, the skill cursor c (0-17), carries the slider:

- `slider.frm` (list index 190, 43 by 29), drawn transparently at (592, 16 + 11c);
- a plus button, `splsoff.frm` / `splson.frm` (193 / 194, 16 by 12), at (614, 20 + 11c);
- a minus button, `snegoff.frm` / `snegon.frm` (191 / 192, 17 by 12), at (614, 31 + 11c).

Both buttons are transparent and play `ib1p1xx1` on press, with no release sound.

- A press in the skill rows area (370, 27) 223 by 195 moves the cursor to row `trunc((y - 27) * 0.092307694)`, at most 17. It also selects that skill's card and moves the slider and buttons.
- The SKILLS caption area (376, 5) 223 by 20 selects the Skills card. It does not move the cursor.

**Spending skill points.** Each step acts on the cursor skill.

- **Plus.**
  - With no unspent points, refusal A.
  - Otherwise, with the skill value at 200 or more, refusal B.
  - Otherwise one unspent point moves into the skill's invested points. The shown value rises as SPEC-006 §3.3 gives: 1% per point, 2% for a tagged skill.
- **Minus.**
  - If the skill value is not above the value it had when the screen opened, refusal C.
  - Otherwise, if the skill has no invested points, refusal C.
  - Otherwise one point returns to the unspent pool, with no upper limit.
- **After every attempt**, the skill's card is selected and the skill list redrawn. The big number at (522, 228) shows the unspent points. After a successful step it animates from the previous value (section 3.8, "Big numbers").
- **Refusals.** Each plays `iisxxxx1` and opens a large dialog box at (192, 126) with title and body in key 32328 (section 3.13):

  | Refusal | Title | Body |
  | --- | --- | --- |
  | A | "No free skill points available." (`editor.msg` 136) | None |
  | B | The skill name followed by ":" | "At maximum level." (132), "Unable to increment it." (133) |
  | C | The skill name followed by ":" | "At minimum level." (134), "Unable to decrement it." (135) |

- **Holding** a slider button repeats exactly as the SPECIAL buttons do in creation mode (section 3.9). It stops on release, on leaving the button, or on a refusal.
- **Keys.** `+`, Right arrow and uppercase `N` make one plus step; `-`, Left arrow and uppercase `J` make one minus step. Keys do not repeat beyond the keyboard's own repeat.

The decrement floor is the skill's value when the screen opened, not its invested points. A value raised during the same visit by Tag! or by losing Gifted through Mutate! therefore lets the player refund points invested on earlier visits. `[May-improve]`: Safe rule: never refund below the invested points the skill had when the screen opened.

**Folder tabs.** One of three images is drawn opaquely at (11, 327), 318 by 33:

- `perksfdr.frm` (list index 180) when Perks is selected;
- `karmafdr.frm` (178) for Karma;
- `killsfdr.frm` (179) for Kills.

When the screen opens, the tab captions are drawn into copies of these images in font 103 on row 5. PERKS (`editor.msg` 109) is centered on x = 61, KARMA (110) on 159 and KILLS (111) on 257, each at x = center - trunc(width / 2). The selected tab's caption is key 18979, the other two key 14723. With the English widths 33, 39 and 30, the captions start at x = 45, 140 and 242 in the image, which is screen (56, 332), (151, 332) and (253, 332).

**Clicking a tab.** A transparent button with mask `fldrmask.frm` (list index 186, 318 by 33) covers (11, 327). On release it plays `ib3p1xx1` and picks the folder from the screen x of the pointer:

| Screen x | Folder | General card selected |
| --- | --- | --- |
| 208 or more | Kills | Kills |
| 111-207 | Karma | Karma |
| 110 or less | Perks | Perks |

The folder and the card are redrawn even when the tab was already selected. Every folder draw first restores the list area (34, 360) 280 by 120 from the background.

**Folder lists.** All folder text is font 101, key 992, or 32747 for the selected folder line. A press in the area (28, 363) 283 by 105 selects folder line `trunc(max(0, y - 364) / 11)` (0-9). In the Kills folder, 10 is added when the screen x is above 174. Folder line j is info line 10 + j.

- **Perks.**
  - Rows start at (34, 364) and step 11. Each perk whose rank is above 0 gets a row, in perk-index order 0-63. That includes the effect perks 53-63, which drugs and armor grant (SPEC-011 §3.22).
  - A row reads "Name" (`perk.msg` 101 + perk), or "Name (r)" when the rank r is above 1. At most 7 rows are drawn; further perks are not shown.
  - Row j is folder line j.
  - The heading "TRAITS" (`editor.msg` 156), of width w, is drawn at y = 439, x = trunc((280 - w) / 2) + 31.
  - Two one-pixel rules in key 992 sit at y = 444. One runs from x = 34 to x = trunc((246 - w) / 2). The other runs from x = trunc((280 - w) / 2) + w + 45 for trunc((246 - w) / 2) pixels more. For the English w = 42 the text starts at x = 150, and the rules span 34-102 and 206-308.
  - The first trait slot is drawn at y = 450 and lights for folder line 8. The second slot is drawn at y = 461 when the first slot is in use, otherwise at y = 450, and lights for folder line 9. Names come from `trait.msg` 100 + trait.
- **Karma.**
  - Row 0 at (34, 362) is "Reputation (General) " (`editor.msg` 1000, with its trailing space) followed by campaign variable 155 in signed decimal.
  - Rows 1 and on start at y = 373 and step 11. They are the entries below whose campaign variable is not 0, in this order. Row j is folder line j.

    | Entry | Variable | Text | Picture |
    | ---: | ---: | --- | --- |
    | 1 | 156 | 1001 "Berserker" | 49 `repbrsrk.frm` |
    | 2 | 157 | 1002 "Champion" | 51 `repgood.frm` |
    | 3 | 158 | 1003 "Childkiller" | 50 `repchild.frm` |
    | 4 | 189 | 1004 "Nuka Cola Craving" | 52 `alchohol.frm` |
    | 5 | 190 | 1005 "Buffout Craving" | 53 `addict.frm` |
    | 6 | 191 | 1006 "Mentats Craving" | 53 `addict.frm` |
    | 7 | 192 | 1007 "Psycho Craving" | 53 `addict.frm` |
    | 8 | 193 | 1008 "RadAway Craving" | 53 `addict.frm` |
    | 9 | 194 | 1009 "Alcohol Craving" | 52 `alchohol.frm` |

    Entry e's card has title `editor.msg` 1000 + e and description 1100 + e. Row 0's card is picture 48 `rep.frm`, title 1000 and description 1100. Pictures are `art/skilldex/` list indices.
- **Kills.**
  - The 15 kill types have names in `proto.msg` 450-464: Men, Women, Children, Super Mutants, Ghouls, Brahmin, Radscorpions, Rats, Floaters, Centaurs, Robots, Dogs, Manti, DeathClaws, Master.
  - Types with a nonzero kill count are listed, sorted by name in byte order, so "DeathClaws" sorts before "Dogs".
  - Rows start at y = 362 and step 11. The first 10 entries fill the left column: name at x = 34, count at x = 136. The rest fill the right column from y = 362: name at x = 191, count at x = 293.
  - Counts are plain decimal. Left row i is folder line i; right row i is folder line 10 + i.

**Condition lines.** They use the positions and colors of section 3.8. A line is lit (key 992, or 32747 when selected) when its condition holds, and dark (key 1313, or 15845 when selected) otherwise. The same test runs in creation mode, where all of them are normally dark.

| Line | Text | Lit when |
| --- | --- | --- |
| 1 | Poisoned (`editor.msg` 312) | The player's current poison level is not 0 |
| 2 | Radiated (313) | The current radiation level is not 0 |
| 3 | Eye Damage (314) | Persistent effect flag `0x40`, blinded, is set (SPEC-007 §2.4) |
| 4 | Crippled Right Arm (315) | Flag `0x20` |
| 5 | Crippled Left Arm (316) | Flag `0x10` |
| 6 | Crippled Right Leg (317) | Flag `0x08` |
| 7 | Crippled Left Leg (318) | Flag `0x04` |

**Info lines and hot regions in game.** A press selects an info line, then redraws the lists, the level block, the folder and the card. These regions are the same as in section 3.8: SPECIAL (19, 38), the conditions, the derived list, the SKILLS caption, the skill rows and the skill-points caption (396, 228) 171 by 25. In game the creation-only trait regions are replaced as follows:

| Region | Info line |
| --- | --- |
| (28, 280) 124 by 32 | 7 + `trunc(max(0, y - 280) / 10)` |
| (28, 363) 283 by 105 | Folder line, above |
| Folder tabs | The folder's general card, above |

Three regions can yield a line past their own block, because their formulas are not clamped:

- the level block gives 10 for y = 310-311, the Perks folder's first line;
- the conditions region gives 51 for y = 145-150, the Armor Class card;
- the derived region gives 61 for y = 305-309, the Small Guns card.

`[May-improve]`: Safe rule: clamp to 9, 50 and 60 respectively. The condition and derived cases also apply in creation mode.

**Info card mapping.** The card geometry is that of section 3.8. The picture is the `art/skilldex/` list index; every corpus picture is 140 by 117. "Base: NN% formula" is drawn after the title in font 101, black, at (348 + title width + 8, 286).

| Info line | Card | Picture | Title | Description |
| --- | --- | --- | --- | --- |
| 0-6 | SPECIAL k | k (`strength.frm`-`luck.frm`) | `stat.msg` 100 + k | `stat.msg` 200 + k |
| 7, creation | Character points | 7 `level.frm` | `editor.msg` 120 | 121 |
| 7, game | Level | 7 `level.frm` | `stat.msg` 401 | `stat.msg` 501 |
| 8, game | Experience | 8 `exper.frm` | `stat.msg` 402 | `stat.msg` 502 |
| 9, game | Next level | 9 `levelnxt.frm` | `editor.msg` 122 | 123 |
| Perks folder, line j below the perk count | j-th held perk p | 72 + p | `perk.msg` 101 + p | `perk.msg` 201 + p |
| Perks folder, line 8 or 9 with at least one trait | Trait in slot j - 8 | 55 + trait | `trait.msg` 100 + trait | `trait.msg` 200 + trait |
| Perks folder, other lines 7-10 | Optional traits | 54 `traits.frm` | `editor.msg` 146 | 147 |
| Perks folder, any other line | Perks | 71 `perks.frm` | 124 | 127 |
| Karma folder, line below the row count | Karma entry | Table above | Table above | Table above |
| Karma folder, other lines | Karma | 47 `karma.frm` | 125 | 128 |
| Kills folder, line below the entry count | Kill type t | 36 `sneak.frm` | `proto.msg` 450 + t, a space, `editor.msg` 126 "Kills" | `proto.msg` 465 + t |
| Kills folder, other lines | Kills | 46 `kills.frm` | 126 | 129 |
| 43 | Hit Points | 10 `hitpoint.frm` | `editor.msg` 300 | `stat.msg` 207 |
| 44-50 | Condition 1-7 | 11-17 (`poisoned.frm` ... `legleft.frm`) | `editor.msg` 312-318 | `editor.msg` 400-406 |
| 51 | Armor class | 18 `armorcls.frm` | `stat.msg` 109 | 209 |
| 52 | Action points | 19 `actionpt.frm` | 108 | 208 |
| 53 | Carry weight | 20 `carryamt.frm` | 112 | 212 |
| 54 | Melee damage | 21 `meleedam.frm` | 111 | 211 |
| 55 | Damage resistance | 22 `damresis.frm` | 124 | 224 |
| 56 | Poison resistance | 23 `poisnres.frm` | 132 | 232 |
| 57 | Radiation resistance | 83 `radresis.frm` | 131 | 231 |
| 58 | Sequence | 24 `sequence.frm` | 113 | 213 |
| 59 | Healing rate | 25 `healrate.frm` | 114 | 214 |
| 60 | Critical chance | 26 `critchnc.frm` | 115 | 215 |
| 61-78 | Skill s | 28 + s (`gunsml.frm` ... `outdoors.frm`) | `skill.msg` 100 + s, then "Base:" (`editor.msg` 137), a space, the fixed base NN, "%", a space, `skill.msg` 300 + s | `skill.msg` 200 + s |
| 79, creation | Tag skills | 27 `skills.frm` | `editor.msg` 144 | 145 |
| 79, game | Skill points | 27 `skills.frm` | 130 | 131 |
| 80 | Skills | 27 `skills.frm` | 150 | 151 |
| 81, creation | Optional traits | 27 `skills.frm` | 146 | 147 |
| 82-97, creation | Trait t | 55 + t (`fastmeta.frm` ... `gifted.frm`) | `trait.msg` 100 + t | `trait.msg` 200 + t |

Notes on the table:

- **Skill bases**, in skill order 0-17: 35, 10, 10, 65, 55, 40, 30, 15, 25, 20, 20, 20, 25, 20, 25, 20, 20, 5. An example title is "Small Guns" followed by "Base: 35% + AG".
- **Perks folder lines.** "Below the perk count" means j is less than the number of held perks, counted up to 7. Trait slot j - 8 is used only when at least one trait is held.
- **Folder general cards.** Selecting a folder tab sets folder line 30 (Perks), 32 (Karma) or 31 (Kills). Those lines show the folder's general card.
- **Kill card art.** The kill-type card uses `sneak.frm` (36), not `kills.frm`. `[Must-match]`

Three lookups go wrong:

- Perks 53-63 have no picture. Their index, 72 + p, lies past the end of `skilldex.lst`, so the card area is cleared and left empty, with no title and no sound. `[May-improve]`: Safe rule: draw the title and description with no picture.
- With exactly one trait held, folder line 9 reads an empty trait slot. `[May-improve]`: Safe rule: show the Optional traits card.
- With an empty first slot and a used second slot, the trait is drawn on line 8's row but lights and hit-tests as line 9. `[May-improve]`: Safe rule: draw slot 1 at y = 461 always.

**Perk chooser.**

| Element | Value |
| --- | --- |
| Window | Screen (33, 91) 573 by 230, modal; background `perkwin.frm` (list index 86, 573 by 230), copied opaquely |
| Heading | "PICK A PERK" (`editor.msg` 152), font 103, key 18979, at (49, 16) |
| Done, Cancel | Red buttons at (48, 186) and (153, 186); captions DONE (100) at (69, 186) and CANCEL (102) at (171, 186), font 103, key 18979 |
| Scroll arrows | Up `uparwoff.frm` / `uparwon.frm` (199 / 200, 11 by 14) at (25, 46); down `dnarwoff.frm` / `dnarwon.frm` (181 / 182, 11 by 14) at (25, 61). Transparent; press sound `ib1p1xx1`, no release sound |
| List | Area (45, 43) 192 by 129, restored before each draw. Up to 11 rows, font 101, row r at (45, 43 + 12r). The highlighted row is key 32747, the others 992. A perk already held shows "(r)" at x = 207 |
| Card | The right part of the window, from x = 280, restored before each draw. Details below |

**Chooser card.**

- Picture: skilldex 72 + perk at (413, 64).
- Title: font 102, black, at (280, 27). For a held perk, "(r)" follows in font 101, black, at (288 + title width, 41).
- Rules: black, at y = 55 and 56 from x = 280 to x = 545.
- Description: `perk.msg` 201 + perk in font 101, black, from (280, 70), stepping 11. It wraps at `133 + max(0, min(150, c) - 8)`, with c as in section 3.8.
- A change of picture or title plays `isdxxxx1`, except for the first card of each list.

**Entries.** The list holds every eligible perk (SPEC-011 §3.21), sorted by name in byte order. Let n be the count. The selected entry is top + row, with top in 0 to max(0, n - 11) and row in 0-10. It starts at entry 0.

**Moving the selection.**

- **Up**, by the Up arrow key or one step of the up button: if top is above 0, top decreases by 1; otherwise row decreases by 1, but not below 0.
- **Down**, with n above 11: if top is below n - 11, top increases by 1; otherwise row increases by 1, up to 10. With n at most 11, row increases by 1, up to n - 1.
- **Home** sets top and row to 0.
- **End**, with n above 11, sets top to n - 11 and row to 10. Otherwise it sets row to n - 1.
- **Holding** a scroll button steps on press. From the 15th 41-millisecond frame it steps at 5 per second, rising by 1 per step to 24 per second, until release or leaving the button. These are the age-arrow timings of section 3.10.
- **A click** in the list area, on release, sets row = `trunc((screen y - 134) / 12)`, clamped to 0 through n - 1.
  - A click on the same row as the previous click chooses that entry and plays `ib1p1xx1`.
  - Any navigation key or scroll step forgets the previous click.
  - So does the idle timer. Whenever no input arrives and more than 700 milliseconds have passed since the timer last restarted, the timer restarts and the click is forgotten. Clicks do not restart it.

**Ending the chooser.**

- **Done** (button), Return (`ib1p1xx1`) or a double click chooses the selected perk.
- **Esc**, Cancel or a pending quit request closes the chooser without a choice. The pick stays pending for the next visit.
- **A choice** adds one rank (SPEC-011 §3.21). If adding fails, the pick stays pending. Otherwise the pending pick is cleared and the immediate extras of SPEC-011 apply. Tag! and Mutate! run their steps in the same window, below.
- **Afterwards** the window closes, and the skills, SPECIAL rows, derived stats, Perks folder and card are redrawn.
- **One pick per opening.** The chooser grants exactly one pick each time it opens.

With no eligible perk the list is empty. The card and a Done choice then use a stale entry left from an earlier list. `[May-improve]`: Safe rule: draw the empty list, ignore Done and double clicks, and keep the pick pending.

**Tag! and Mutate!** These exist only through the chooser, by picking the perk. The window stays open. Before each step the heading strip at (49, 15), 206 by 22, is restored from the art and the step's heading is drawn at (49, 16). The restore reads the art one row higher than the strip it covers. `[May-improve]`: Safe rule: restore from the same rows. Each step uses the chooser's list, keys and double click, with the card showing the entry's own picture, name and description and no rank.

| Step | Heading | Entries | Choice | Esc or Cancel |
| --- | --- | --- | --- | --- |
| Tag! | "PICK A NEW TAG SKILL" (155) | Skills not tagged, sorted by name (15 with three tags); card picture 28 + skill | The skill fills the fourth tag slot and gains the tag bonus at once | Tags restored; Tag! rank removed |
| Mutate!, step 1, only when at least one trait is held | "LOSE A TRAIT" (154) | Held traits, sorted by name | That trait is removed; go to step 2 | Traits restored as at screen open; Mutate! rank removed |
| Mutate!, step 2 | "PICK A NEW TRAIT" (153) | The 16 traits minus those held when the screen opened, sorted by name | The trait is applied at once | Same as step 1 |

Two defects:

- When the player held any trait at the start of Mutate!, the new trait goes into the second slot. A player who held exactly one trait is left with an empty first slot. `[May-improve]`: Safe rule: put the new trait into the first empty slot.
- Cancelling either step still consumes the pending pick, although the rank is removed. `[May-improve]`: Safe rule: leave the pick pending, as when the chooser closes without a choice.

**Keys in game.**

| Key | Action |
| --- | --- |
| Return, `d`, `D` | Done; the keys play `ib1p1xx1` |
| Esc | Cancel |
| `p`, `P` | Print to file (section 3.43) |
| `+`, Right, uppercase `N` / `-`, Left, uppercase `J` | One skill plus / minus step |
| Ctrl+Q, Ctrl+X, F10 | Quit confirmation; Yes cancels on the next frame |
| F12 | Screenshot |

The creation keys `n`, `a`, `s` and `o` do nothing in game.

**Done and Cancel.** Done (button, Return, `d`, `D`) closes with no validation and keeps every change. Cancel (button, Esc, or a pending quit request) restores the whole open snapshot. That undoes:

- spent skill points;
- a chosen perk with its extras, such as the Lifegiver HP, the Educated points, a Tag! skill or a Mutate! trait;
- the level reconciliation, together with the last reconciled level and the pending-perk flag.

The reconciliation and the chooser therefore repeat on the next visit. Perks are restored by removing ranks above the saved ones, then adding ranks up to them, so their stat effects follow. Current HP returns to its value at open. No exit heals the player in game.

**`editor.msg` usage.** These IDs answer which caption is used where. Each text is the English corpus string.

| ID | Text | Use |
| --- | --- | --- |
| 100, 101, 102 | DONE, OPTIONS, CANCEL | Editor buttons (creation uses OPTIONS); DONE and CANCEL also in the perk chooser and the name/age/sex dialogs |
| 103 | PRINT | In-game caption of the options button |
| 104, 107, 108 | AGE, MALE, FEMALE | Age and sex captions |
| 105, 106 | SEX, NAME | Not used |
| 109, 110, 111 | PERKS, KARMA, KILLS | Folder tab captions, in game |
| 112 | SKILL POINTS | Caption at (400, 233), in game |
| 113, 114, 115 | "Level:", "Exp:", "Next Level:" | Level block, in game |
| 116 | CHAR POINTS | Creation caption |
| 117 | SKILLS | Skills caption, both modes |
| 118-119, 142-143, 157-158 | Validation pairs | Creation Done (section 3.9) |
| 120-121 | Character Points | Creation card, line 7 |
| 122-123 | Next Level | In-game card, line 9 |
| 124/127, 125/128, 126/129 | Perks, Karma, Kills | Folder general cards; 126 also ends the kill-type card title |
| 130-131 | Skill Points | In-game card, line 79 |
| 132-136 | Refusals | Skill plus and minus, in game |
| 137 | "Base:" | Skill card attribute text, both modes |
| 138, 139 | TAG SKILLS, OPTIONAL TRAITS | Creation captions |
| 140-141, 148-149 | Refusal pairs | Creation tag and trait buttons |
| 144-145, 146-147, 150-151 | Tag Skills, Optional Traits, Skills | Cards; 146-147 also for the Perks-folder trait lines |
| 152, 153, 154, 155 | PICK A PERK, PICK A NEW TRAIT, LOSE A TRAIT, PICK A NEW TAG SKILL | Chooser headings |
| 156 | TRAITS | Perks-folder heading |
| 200-209, 300-318, 400-406 | As section 2.4 | Both modes |
| 500-511 | Month names | Printed sheet date line |
| 600-618 | Options dialog and file messages | Sections 3.11 and 3.43; 608, 613, 614 and 618 are not used |
| 620-655 | Printed sheet | Section 3.43 |
| 1000-1009, 1100-1109 | Karma names and descriptions | Karma folder |

The corpus file also holds 1110 and 5000-5007, which are demo and test texts, and seven entries numbered 000, which are layout templates. None of them is ever looked up.

### 3.43 Character Sheet Save, Load and Print

`[Must-match]` unless tagged.

**Entry points.**

| Where | Control | Operations |
| --- | --- | --- |
| Creation mode, Options dialog (section 3.11) | SAVE (`s`), LOAD (`l`), PRINT TO FILE (`p`) | Save a `.GCD` sheet, load one, print the sheet |
| In game | PRINT Red button at (343, 454), or `p` / `P` | Print only. There is no Options dialog, and SAVE, LOAD and ERASE do not exist in game |

**File list.** Each operation first builds a list of names for its pattern, `*.GCD` or `*.TXT`. The list is the union of two sources:

- every entry in the root directory of the master archive whose stored name is longer than 4 characters and whose last three characters equal the extension (the dot is not checked);
- every loose file matching the pattern in the master patch root: the `fallout.cfg` key `master_patches` in section system, default "data" (SPEC-008 §3.2).

The names are merged into one list, sorted case-insensitively. A name found in both sources appears once. Names are shown as stored, with their extension. The corpus archive root holds no `.GCD` or `.TXT` files (the premades are under `premade/`), so in practice the list shows files the player wrote earlier.

- The reference reserves 13 bytes per name, so a loose file whose name is longer than 12 characters overflows the list. `[May-improve]`: Safe rule: leave such names out.
- The list builder cannot fail. The "Error reading file list!" boxes (`editor.msg` 615) of sections 3.11 and below are therefore never shown.

**Dialog geometry.** Both file dialogs open at screen (168, 80) as modal windows.

| Element | Load dialog | Save dialog |
| --- | --- | --- |
| Background | `loadbox.frm` (list index 224, 288 by 229) | `savebox.frm` (list index 225, 288 by 257) |
| Title | Font 103, key 18979, at (49, 16) | Same |
| File list | Area (55, 49) 190 by 124, filled with raw index 100 before each draw. Up to 12 rows, font 101, row r at (55, 49 + 10r). The highlighted row is key 32747, the others 992 | Same |
| Scroll arrows | Up `uparwoff.frm` / `uparwon.frm` at (36, 44); down `dnarwoff.frm` / `dnarwon.frm` at (36, 58); 11 by 14, transparent | Same |
| Done | Red button at (58, 187); "DONE" (`dbox.msg` 100) at (79, 187) | Red button at (58, 214); caption at (79, 213) |
| Cancel | Red button at (163, 187); "CANCEL" (`dbox.msg` 103) at (182, 187) | Red button at (163, 214); caption at (182, 213) |
| Name field | None | (57, 190), font 101, key 992 text on raw index 100 |

Captions are font 103, key 18979. Done and Cancel use the Red-button sounds. The reference attaches the scroll-arrow sounds to Cancel again, so the arrows are silent. `[May-improve]`: Safe rule: give the arrows `ib1p1xx1` on press.

**Shared list rules.** The selected file is entry top + row, starting at entry 0. Let n be the number of files. The list scrolls when n is above 12. Let M be the maximum top: n - 13 when n is above 12, otherwise unused.

- **Up**, by the key or one arrow step: top decreases by 1; at 0, row decreases by 1 instead, but not below 0.
- **Down by an arrow step**, when scrolling: top increases by 1; past M, top stays at M and row increases by 1, up to 11. Without scrolling, row increases by 1, up to n - 1.
- **Down by the key** follows the same rule, except as follows.
  - The load dialog lets row reach 12, which is not drawn, before it clamps to 11. A Done in that state takes entry top + 12.
  - The save dialog moves the row on the very press that reaches M.
- **Home** sets top and row to 0.
- **End**, when scrolling, sets top to M and row to 11. Otherwise it sets row to n - 1 and top to 0.
- **Holding** an arrow repeats with the timings of the perk chooser's arrows, until release or leaving the arrow.
- **A click** in the list area, on release, sets row = `trunc((screen y - 80 - 49) / 10)`, at least 0. When scrolling, the row is at most 11; otherwise a row past the last file becomes n - 1. With n = 0 clicks do nothing.
  - A second click on the same row chooses that file and plays `ib1p1xx1`.
  - Any key navigation or arrow step forgets the previous click.
  - So does a free-running counter of idle 41-millisecond frames, every 32 frames. Clicks do not restart that counter.

Because M is n - 13, the last file of a list longer than 12 is never drawn. `[May-improve]`: Safe rule: let M be n - 12, and keep the row within 0-11 and within the list.

**Load dialog input.** There is no name field.

- Done with a nonempty list returns the selected name, extension included.
- Done with an empty list, Cancel, Esc or a pending quit request cancels.
- Return does nothing.

**Save dialog input.**

- **Initial field.** The field starts with the default name, cut to its first 12 characters, and is drawn with one trailing space.
- **Typing.** A typed key is appended while the field holds fewer than 8 characters, when it is a code 32-122 that is an ASCII letter or digit or one of `` # @ ! $ ` ' ~ ^ & ( ) - _ = [ ] { } ``. Space is not accepted.
- **Erasing.** Backspace or Delete removes the last character.
- **Cursor.** A 4-by-8 block at the end of the trailing space toggles every 3 frames between key 992 and raw index 100. While an arrow is held, the blink state changes but is not shown until the next redraw.
- **Single click.** A click on a row puts that file's name into the field, cut at its first `.` and to at most 12 characters. Typing is then blocked until erasing brings the field below 8 characters.
- **Double click.** It returns that file's base name at once.
- **Accept.** Done, or Return (`ib1p1xx1`), returns the field. An empty field cancels.
- **Cancel.** Cancel, Esc or a pending quit request cancels.

**Where files go.** The returned name gets `.GCD` or `.TXT` appended. Files are always written as loose files in the master patch root, never into the archive (SPEC-008 §3.2).

- **Existence test.** "Exists" means the file opens for reading, either as a loose file in that root or as an archive member.
- **Sheet files.** A sheet is written in binary mode, 428 bytes (SPEC-006 §2.2).
- **Printed sheets.** A printed sheet is written in text mode, so every line ends in CR LF (`0D 0A`).
- **Upper case.** Every message shows the file name upper-cased. Upper-casing also renames the file actually written when the overwrite prompt was shown; otherwise the typed case is kept. The reference file system ignores case. `[May-improve]`: Safe rule: on a case-sensitive file system, match existing names without regard to case and write new names in upper case.

**Operations.** In the table, NAME is the upper-cased name with its extension. Every message box is at (169, 126) (section 3.13). An error box plays `iisxxxx1` first.

| Operation | Default name, title | Overwrite prompt | Success | Failure |
| --- | --- | --- | --- | --- |
| Creation SAVE | `editor.msg` 617 "PLAYER", 600 "SAVE" | When the file exists: Yes/No, key 32328, title "NAME already exists." (609), body "Do you wish to overwrite?" (610). No returns to the Options dialog | Commit the working tags and traits, write the sheet, show "NAME was successfully saved." (607), large, key 992, then close the Options dialog | "NAMEError saving !", large, key 32328, as section 3.11; the Options dialog stays open |
| Creation LOAD | 601 "LOAD", load dialog | None | As section 3.11 (no message), then close the Options dialog | "Error loading " (612), the listed name as stored, "!"; key 32328, size by the section 3.13 rule; the Options dialog stays open |
| Creation PRINT TO FILE | 616 "PRINT", 602 "PRINT TO FILE" | Inverted: shown when the file does **not** exist (section 3.11). No returns to the Options dialog | After writing: "NAME was successfully saved.", key 992, size by the section 3.13 rule; then close the Options dialog | "Error saving NAME!" (611 + NAME + "!"), large, key 32328; then close the Options dialog |
| In-game PRINT | 616 "PRINT", 602 "PRINT TO FILE" | When the file exists, as for SAVE. No returns to the character screen | No message; back to the character screen | "Error saving NAME!", large, key 32328 |

A cancelled file dialog returns to the Options dialog, or in game to the character screen, with nothing written. LOAD replaces the player only in creation mode (section 3.11); a game in progress can never load a sheet.

**Printed sheet.** The sheet describes the player as currently shown. In game that includes current HP and effective statistics; in creation mode, level 1. S(n) below is the text of `editor.msg` n. "2d" means at least two digits with leading zeros, and "3d" at least three; a negative value takes a leading `-`, as in `-05`. Numbers are separated by single spaces exactly as listed. The reference writes these lines in order:

1. Two empty lines.
2. S(620) "FALLOUT", S(621) "VAULT-13 PERSONNEL RECORD", and the date line, each preceded by `trunc((80 - L) / 2) - 2` spaces, where L is the line's byte length. A negative count means no spaces.
   - The date line is: the day as 2d, a space, the month name `editor.msg` 500 + month - 1, a space, the year, two spaces, HHMM as 4d, a space, S(622) "hours".
   - The date is the game-calendar date of the save header (SPEC-008 §2.4).
   - HHMM = `100 * (trunc(t / 36000) mod 24) + trunc(t / 600) mod 60`, for game tick t.
3. An empty line.
4. The name line: S(642) "  Name:", a space and the player name, padded with spaces to 27 characters (never cut). Then S(643) "Age:", a space and the age; then S(644), a space and S(645 + gender), "Male" or "Female".
5. The level line: S(647) " Level:", a space, the level as 2d, a space, S(648), a space, the grouped experience X (section 3.42) and a space. Then 12 - length(X) spaces when positive, S(649) "Next Level:", a space and the grouped next threshold, which is empty at level 21.
6. An empty line. The statistics heading S(623) is composed but not written.
7. Seven statistic lines. Each label is followed by a space and the value:

   | Line | Content |
   | ---: | --- |
   | 1 | S(624) Strength 2d, S(625) current HP 3d "/" maximum HP 3d, S(626) **Strength** 2d |
   | 2 | S(627) Perception 2d, S(628) armor class 3d, S(629) healing rate 2d |
   | 3 | S(630) Endurance 2d, S(631) action points 2d, S(632) critical chance 3d "%" |
   | 4 | S(633) Charisma 2d, S(634) melee damage 2d, S(635) carry weight 3d " lbs." |
   | 5 | S(636) Intelligence 2d, S(637) damage resistance 3d "%" |
   | 6 | S(638) Agility 2d, S(639) radiation resistance 3d "%" |
   | 7 | S(640) Luck 2d, S(641) poison resistance 3d "%" |

8. Two empty lines.
9. If the first trait slot is in use: S(650), then one line "  " + trait name for each used slot.
10. If any of the 64 perks has a rank: S(651), then one line per held perk in perk-index order, "  Name" or "  Name (r)" for rank r above 1.
11. An empty line, S(652), and an empty line.
12. S(653). The reference composes a skill row for each skill but writes none of them.
13. Two empty lines, then S(654).
14. Inventory rows. The entries of the player's inventory list are taken in stored order, three per row; worn and held items are included, because they are inventory entries.
    - Each cell is "  ", the grouped quantity, "x ", the item name, right-padded with spaces to 25 characters.
    - A row ends after its third cell or the last entry. Trailing spaces stay.
15. An empty line, then S(655), a space, the total inventory weight (SPEC-011 §3.14) and " lbs.". Then two empty lines.

**Messages for the printed sheet.** R1 looks up IDs 650-655 as: traits heading, perks heading, karma heading, skills-and-kills heading, inventory heading, total-weight label. The English corpus file numbers them differently:

| ID | Corpus text |
| ---: | --- |
| 650 | " ::: Traits :::           ::: Perks :::           ::: Karma :::" |
| 651 | " ::: Skills :::                ::: Kills :::" |
| 652 | " ::: Inventory :::" |
| 653 | 38 spaces, then "Total Weight:" |
| 654, 655 | Absent |

A missing message reads as the text `Error` (SPEC-002). With the shipped file the reference therefore:

- prints the Skills/Kills heading above the perks;
- prints the Inventory heading where the Karma heading belongs;
- prints the Total Weight label alone where the skill rows belong;
- prints `Error` above the inventory;
- prints `Error N lbs.` as the last text line.

Statistic labels 624-641 and the name and level parts 642-649 match the corpus. The corpus template entries numbered 000 show the intended column layout.

The sheet has four defects, and an engine may apply all four safe rules together:

- **Sequence column.** The Sequence column prints Strength. `[May-improve]`: Safe rule: print the sequence value as 2d.
- **Skill rows.** No skill rows are written, and the kill pairing never advances. `[May-improve]`: Safe rule: for each skill s in order, write a row.
  - The row is "  ", the skill name, a space, and dots up to 16 characters in all; then a space, the skill value as 3d and "%".
  - Pair the k-th skill with the k-th kill type, in kill-type order 0-14, whose count is nonzero. For a paired skill, append 8 spaces, the kill-type name and a space, dots up to 16 characters, a space, and the count as 3d.
- **Headings.** The headings 650-655 do not match the shipped file. `[May-improve]`: Safe rule: assign the shipped texts by meaning.
  - Write 650 once before the trait and perk lines, which follow with no further heading. That trait-and-perk block is written when any trait slot or perk is in use.
  - Write the skill rows under 651, after two empty lines.
  - Write the inventory rows under 652, after two empty lines.
  - End with 653, a space, the weight and " lbs.".
- **Next Level at level 21.** Next Level is empty at level 21. `[May-improve]`: Safe rule: print `------`, as on screen.

Fixture P gives the literal reference output and the safe-rule variant.

### 3.44 Map Object Draw Order, Lighting and Translucency

**Frame layers.** A map refresh of a screen rectangle on the current elevation draws, in this order: floor tiles; the map objects in the two passes below; roofs (section 3.36); the outlines collected during the passes; floating text; and objects that have no hex, at full light 65536. `[Must-match]`

**Per-hex object lists.** Every hex keeps its objects in one ordered list. `[Must-match]`

- The list is sorted by elevation, lowest first. Within one elevation, flat objects (object flag `0x00000008`, SPEC-003 §2.6) come before all other objects.
- Within one elevation and flat class, a newly inserted object goes **after** every object already in that group, so the oldest insertion comes first. R1 locks both objects' art at this point, but its comparison is incomplete and never decides anything. No art, frame size, object type or identifier breaks the tie.
- An object is inserted when it is loaded with its map (in file order), created, copied, moved to a hex, flipped between flat and not flat, or given a new pixel offset. Each of these moves the object to the end of its group on its hex, even when the hex does not change. R1 never compares the new state with the old one:
  - A move to a hex always removes the object and inserts it again, even when the hex and elevation are the ones it already has. The move also resets the pixel offset to (0, 0).
  - A pixel-offset change always removes and reinserts the object, even when the change is (0, 0).
  - Every frame step of an animation applies that frame's shift as an offset change, even a zero shift. This covers every step of a walk (SPEC-007 §3.11), every forward or reverse frame of any animation, and the first frame of a new animation. An animated object, such as a burning fire or a fidgeting critter, therefore moves to the end of its group on every frame it advances.
  - A walking critter therefore draws after the other objects of its class on its hex.
  - These do not reinsert: an art change, setting or stepping the frame outside an animation step, a rotation change, a light change, hiding or showing, and an outline change.

**Visited hexes.** `[Must-match]` The objects drawn are found through a fixed hex lattice, which is built once at engine start for the 640-by-380 map window widened by 320 pixels on each side and 240 above and below.

- The lattice has W = 41 columns and H = 72 rows: `trunc((960 + 320 + 1) / 32) + 1` and `trunc((620 + 240 + 1) / 12) + 1`.
- Two tables are built, one for each parity of the view-center hex, with the view centered on a hex of that parity. Let O be the hex under window point (-320, -240), and (X0, Y0) its window position plus (16, 8). Let s = 16, or -16 when X0 is greater than -320. Lattice row y has its points at y-coordinate Y0 + 12y. Its first point is at x = X0 for row 0, X0 + s for odd rows, and X0 - s for even rows from 2 on, and the next points follow every 32 pixels. Each point stands for the hex under it. The table holds each point's hex minus O, and those offsets, sorted ascending, give the visiting order.
- A refresh of rectangle R uses the table for the current center's parity. It widens R by 320 pixels left and right and 240 up and down, and takes the hex under the widened top-left corner as the base. It visits base plus offset for every lattice point whose row is below `trunc(height / 12)` and whose column is below `trunc(width / 32)`, the height and width being those of the widened rectangle. Hexes are visited in ascending offset, which is ascending hex number.
- An object whose hex lies outside the visited set is not redrawn, even when its sprite reaches into R. Every drawn object is clipped to R.

**Two passes.** `[Must-match]`

1. **Flat pass.** For each visited hex in order, walk its list. Skip objects on lower elevations, and stop at the first object on a higher elevation. On the current elevation, draw each flat object that is not hidden (flag `0x00000001`). Stop at the first non-flat object and remember it.
2. **Upright pass.** For each remembered object, in the same hex order, draw it and every later object on the current elevation in that hex's list, skipping hidden ones.

So every flat object in the refreshed area is drawn before any upright object. Upright objects are drawn by ascending hex number and, within a hex, in list order. The hex number alone sets the order of objects on different hexes: no screen-y sort, sprite-bottom sort or multihex adjustment is made.

**Light and outlines.** `[Must-match]` Each hex's objects are lit by the larger of the ambient light and that hex's light on the current elevation (SPEC-006 §3.6), the hex light being capped at 65536. Every drawn object with an outline type set and its outline not disabled is added to the outline list, in drawing order. Objects without a hex are never outlined. The list holds at most 100 objects per refresh; later objects are not outlined in that refresh. Outlines are drawn after the roofs, in list order, so an outline shows through a roof that hides its object.

**Outline word.** Every object has a 32-bit runtime outline word. `[Must-match]`

| Bits | Meaning |
| --- | --- |
| 0-23 | Outline type. 0 is no outline. The value is compared whole: 1 hostile, 2 red, 4 grey, 8 friendly, 16 item. Any other nonzero value draws in key 31775 [49], opaque |
| 30 (`0x40000000`) | Translucent: honored for types 2, 4 and 16 only |
| 31 (`0x80000000`) | Disabled: the type is kept, but the object is not outlined |

These operations change the word:

- **Set a type.** This is refused when the word already has a type, or when the object has the no-highlight flag `0x00001000`. Otherwise the word becomes the new type. The disabled bit is added when the object is hidden (flag `0x00000001`).
- **Enable.** Clears the disabled bit. Showing a hidden object also clears it.
- **Disable.** Sets the disabled bit, but only when a type is set. Hiding an object also does this.
- **Remove.** Sets the word to 0.

An object loaded from a map or save always starts with the word 0 (SPEC-003 §2.10). Saves write the current word.

**Outline colors.** `[Must-match]`

| Type | Color | Translucent form (bit 30) |
| --- | --- | --- |
| 1 hostile | Raw indexes 243-247 (the fast-fire cycle, SPEC-001 §3.2), in row bands | Never |
| 8 friendly | Raw indexes 229-232 (the slime cycle), in row bands | Never |
| 2 red | Key 31744 [133] | The red blend table of the red translucency color |
| 4 grey | Key 15855 [9] | The blend table of the wall translucency color; no R1 caller uses type 4 |
| 16 item | Key 30632 [58] | The red blend table |
| Other | Key 31775 [49] | Never |

**Row bands** (types 1 and 8). Let n be 5 for hostile, or 4 for friendly, and b = `trunc(frame height / n)`. When b is 0, every outline pixel uses the first index, 243 or 229. Otherwise a pixel painted while scanning frame row y uses index `first + ((trunc(y / b) + 1) mod n)`. So rows 0 through b - 1 use the second index, and the first index comes last. Because these indexes cycle in the palette (SPEC-001 §3.2), the outline also shimmers over time.

A translucent outline pixel is `blend[g][D]`. Here D is the destination index at that pixel, g is the common gray level (above) of the outline color's index, and blend is the table named in the table above. No light is applied. For the red type, index 133 has gray level 2.

**Outline geometry.** The outline is a one-pixel ring of the object's current frame (art, frame and rotation), at the same screen position as the object. The ring is every pixel that is transparent in the frame, or lies outside the frame, and has a non-transparent frame pixel directly left of it, right of it, above it or below it. Diagonal neighbors do not count. It is painted over whatever is already in the buffer: the object, its neighbors and the roofs. In frame-local coordinates (x right, y down), R1 paints it in two passes. `[Must-match]` unless tagged.

1. **Rows.** Each row y is scanned from x = 0, starting in the "outside" state.
   - At a non-transparent pixel in the outside state, paint (x - 1, y) and switch to the inside state.
   - At a transparent pixel in the inside state, paint (x, y) and switch to outside.
   - If the row's last pixel (frame width - 1) is non-transparent, paint (frame width, y).
2. **Columns.** Each column x is then scanned from y = 0 in the same way.
   - At a non-transparent pixel in the outside state, paint (x, y - 1).
   - At a transparent pixel in the inside state, paint (x, y).
   - If the column's last pixel is non-transparent, paint (x, frame height).
3. **Repaints.** A ring pixel next to the sprite both horizontally and vertically is painted twice, and the column pass's paint is the one that stays.
   - A translucent outline blends a second time over its own first result.
   - In a banded outline, a pixel painted above a column's run takes the band of the row below it.
4. **Clipping.** Each paint happens only when the scanned pixel that triggers it lies inside the refreshed rectangle widened by 1 pixel on every side and limited to the map buffer. The scanned pixel is the non-transparent one for left, above and end-of-row paints, and the transparent one otherwise. So a ring pixel can land up to 2 pixels outside the refreshed rectangle. That pixel stays in the buffer until a later refresh covers it.
5. **Buffer guards.**
   - A left paint is skipped when the scanned pixel is the first byte of the buffer or lies in buffer column 0.
   - An above paint is skipped when it would fall before the buffer start.
   - The end-of-row and end-of-column paints are skipped when the scanned pixel is not inside the buffer.
   - An end-of-row paint for a sprite whose last column is buffer column 639 lands in column 0 of the next buffer row. `[May-improve]`: skip a right paint that would wrap to the next row.

**When outlines are set.** `[Must-match]` Item hover (section 3.27) sets type 16 without the translucent bit. The movement-mode hex cursor sets type 2 with the translucent bit (section 3.27). Combat uses types 1 and 8:

- **Combat start.** Every combatant gets type 1, or type 8 when the player has Friendly Foe and the combatant's team is the player's team (SPEC-011). The outline is set disabled.
- **A critter joining combat or changing teams** gets its type rebuilt (SPEC-009 §3.15, step 4).
- **The player's turn.** When the `target_highlight` setting read at combat start is nonzero, outlines are enabled at the start of the turn. Enabling needs the setting to be nonzero now and the cursor to be in crosshair mode (section 3.27). It enables the outline of every living combatant other than the player. Outlines are disabled again when the turn ends.
  - With setting 2, the outlines are also disabled when one of the player's attack animations starts, and enabled again after its damage is applied.
  - Entering crosshair mode enables them and leaving it disables them, as in section 3.27.
- **An NPC's turn.** The acting NPC's outline is enabled for its whole turn, whatever the setting, and disabled at the end.
- **Death.** It disables the critter's outline.
- **Combat end, or leaving the combat list.** Removes the outline.
- **Animations.** The animation system has a sequence step that enables or disables its owner's outline. No R1 code registers that step: no engine sequence (SPEC-007 §3.10, §3.11) and no script opcode adds it, so it never runs. An engine needs no such step for parity.

Outside combat, the enable step applies to every living critter on the current elevation other than the player. It sets type 8 for the player's team and type 1 for everyone else, without the Friendly Foe test, and enables it. The disable step removes these outlines entirely.

**Floor light.** Floor tiles are lit per pixel by interpolating the light of ten hexes around the tile. Objects still use one light per hex. `[Must-match]` unless tagged.

1. **Base hex.** For a floor tile whose art's top-left is at window (x, y), the base hex is the hex under window point (x, y + 13). If no hex is there, the tile is not drawn.
2. **Vertex lights.** Vertex k, for k = 0-9, has a hex offset from the base hex and a position in a 41-row, 80-column light map. Its light Vk is the larger of the ambient light and that hex's light (SPEC-006 §3.6), where an offset that leaves the grid gives hex light 0.

   | Vertex k | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
   | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
   | Map position (row × 80 + column) | 16 | 48 | 960 | 992 | 1024 | 1936 | 1968 | 2000 | 2912 | 2944 |
   | Hex offset, even base hex | -1 | -2 | 0 | 199 | 198 | 200 | 399 | 398 | 400 | 599 |
   | Hex offset, odd base hex | -201 | -2 | 0 | -1 | 198 | 200 | 199 | 398 | 400 | 399 |

   R1 always uses the even-hex offsets. For an odd base hex it also skips vertex 0, which keeps the light computed for the previous floor tile drawn. `[May-improve]`: compute all ten vertices, with the odd-hex offsets when the base hex number is odd.
3. **Uniform tile.** When all ten lights are equal, the tile is drawn with the lit draw at that light, so indexes 229 and above are not lit.
4. **Interpolated tile.** Otherwise the light map is filled by ten triangles in this order. A later triangle overwrites shared entries. All divisions truncate toward zero.
   - **Five upright triangles** (apex A; base B on the left and C on the right), as (B, C, A): (2, 3, 0), (3, 4, 1), (5, 6, 3), (6, 7, 4), (8, 9, 6).
     - Let dx = `trunc((VC - VB) / 32)` and dy = `trunc((VB - VA) / 13)`.
     - A cursor q starts at A's map position, and a row value s starts at VA.
     - For each of 13 rows: q advances by the row's step, then n entries get s, s + dx, s + 2dx and so on, with q moving past them, and then s grows by dy.
     - The (step, n) pairs are (-1, 2), (78, 2), (76, 6), (73, 8), (71, 10), (68, 14), (65, 16), (63, 18), (61, 20), (58, 24), (55, 26), (53, 28), (50, 32).
   - **Five inverted triangles** (top-left T, bottom apex A, top-right R), as (T, A, R): (0, 3, 1), (2, 5, 3), (3, 6, 4), (5, 8, 6), (6, 9, 7).
     - Let dx = `trunc((VR - VT) / 32)` and dy = `trunc((VA - VT) / 13)`.
     - q starts at T's map position, and s starts at VT. The rows are filled the same way.
     - The pairs are (0, 32), (48, 32), (49, 30), (52, 26), (55, 24), (57, 22), (60, 18), (63, 16), (65, 14), (67, 12), (70, 8), (73, 6), (75, 4).
   - **Drawing.** Tile pixel (px, py) then uses map entry `160 + 80 py + px`. A non-transparent source index i is drawn as intensity[i][trunc(entry / 512)] (SPEC-001 §3.7), with no exemption for indexes 229-255. Their intensity rows are unmapped, so in an interpolated tile those pixels draw as index 0. `[May-improve]`: leave indexes 229-255 unlit, as in the uniform case.
5. **Uncovered entries.** The map persists between tiles and starts as zeros. A few tile pixels read entries no triangle ever writes, so they get light 0. These are pixels (32-34, 35), the bottom tip of 1404 of the 1600 corpus floor tiles, and edge pixels of about 51 others. The tile drawn next, 32 pixels right and 24 down, normally covers them. `[May-improve]`: give those pixels the light of the nearest written entry in the same column above.

**Draw modes.** One draw mode applies to the object's whole clipped rectangle, except where the player cutout of section 3.37 applies. Let s be a source pixel, D the destination index, and L the light level divided by 512 (0-128). The intensity table is the one of SPEC-001 §3.7. A source index 0 is transparent in every mode. `[Must-match]`

- **Lit draw.** The result is s unchanged for s of 229 or more, otherwise intensity[s][L].
- **Translucent draws.** These are selected by the object's six translucency bits (SPEC-003 §2.6): exactly one bit set chooses the row below. With none of the bits set, with only "explicitly no translucency", or with two or more set, the lit draw is used. Two bits can combine, for example when glass translucency is added to a critter by the Stealth Boy.

  | Bit | Foreground color F | Gray table | Light used |
  | --- | --- | --- | --- |
  | Wall translucency `0x10000` | key 25439 [33] | common | Always 65536 (L = 128), whatever the hex light |
  | Glass translucency `0x20000` | key 10239 [35] | glass | The hex light |
  | Steam translucency `0x40000` | key 32767 [220] | common | The hex light |
  | Energy translucency `0x80000` | key 30689 [58] | common | The hex light |
  | Red translucency `0x4000` | key 31744 [133] | common | The hex light |

  - The result is intensity[B][L], with B = blend_F[g][D] and g the gray level of s.
  - blend_F is the per-color blend table of SPEC-002 §3.2. Level 0 gives D, and levels 1 to 7 give the seven-step mix of F over D.
  - With the 5-bit components (r, g, b) of SPEC-001 §3.7:
    - the common gray level is `trunc(trunc((b + 3r + 6g) / 10) / 4)`;
    - the glass gray level is `trunc(trunc((r + 5g + 4b) / 10) / 4)`.
    Both are 0-7, and both are 0 for index 0.
  - A source pixel of gray level 0 leaves the destination color but still relights it at L.
  - Indexes 229-255 get no exemption.

  The gray tables are built once, when the object system starts, from the palette loaded then. The five blend tables are rebuilt with every palette file load (SPEC-001 §3.7). `[Must-match]`

### 3.45 Dialogue Review and Ask About Windows

This section gives the art, layout and input of the dialogue control panel's Review and Tell Me About controls. SPEC-006 §3.9 owns the review history and the Ask About lookup rules, and SPEC-013 §3.18 owns barter. Art is under `art/intrface/`. Sizes were read from the corpus FRM headers. `[Must-match]` unless tagged.

**Control panel.** During dialogue, the panel `di_talk.frm` (list index 99, 640 by 190) is a window at screen (0, 290), 640 by 190. Its art already carries the labels REVIEW, BARTER and TELL ME ABOUT. It has three buttons, each acting on mouse release. Coordinates are window-relative.

| Control | Rectangle | Up / down art | Sounds | Key |
| --- | --- | --- | --- | --- |
| Barter | (593, 41) 14 by 14 | `di_rdbt2.frm` (96) / `di_rdbt1.frm` (95), drawn transparently | `ib2p1xx1` / `ib2lu1x1` | `b` (SPEC-013 §3.18) |
| Tell Me About | (593, 116) 14 by 14 | Same as Barter | Same as Barter | `a` |
| Review | (13, 154) 51 by 29 | `di_rest1.frm` (97) / `di_rest2.frm` (98), drawn opaquely | `ib1p1xx1` / `ib1lu1x1` | None |

Both 14-by-14 buttons use the art listed as "down" (`di_rdbt2.frm`) for their normal state and the "up" art when pressed. The keys `a` and `b` act only in the reply/choice loop of SPEC-006 §3.9. No key opens Review.

**Review window.**

- **Opening.** If talking-head speech is still playing, it is stopped (SPEC-010 §3.12). The head updater is suspended, so the head shows a frozen frame until the window closes. A modal window opens at screen (0, 0), 640 by 480, filled opaquely with `review.frm` (102, 640 by 480). Font 101 is selected.
- **Buttons.** Each acts on release.

  | Button | Rectangle | Up / down art | Sounds | Sends |
  | --- | --- | --- | --- | --- |
  | Scroll up | (475, 152) 35 by 35 | `di_bgup1.frm` (89) / `di_bgup2.frm` (90), transparent | `ib2p1xx1` / `ib2lu1x1` | Up arrow |
  | Scroll down | (475, 191) 35 by 37 | `di_bgdn1.frm` (87) / `di_bgdn2.frm` (88), transparent | `ib2p1xx1` / `ib2lu1x1` | Down arrow |
  | Done | (499, 398) 82 by 46 | `di_done1.frm` (91) / `di_done2.frm` (92), transparent | `ib1p1xx1` / `ib1lu1x1` | Esc |

- **Keys.** Esc closes the window. Up and Down move the top entry by one. Every other key, including Ctrl+Q and F10, is read and ignored.
- **Scrolling.** The first view starts with the oldest entry (entry 0) at the top. Up does nothing at entry 0. Down stops at the last entry, which can then be the only one shown. A press that cannot move does not redraw. Scrolling moves by whole entries, not by lines.

**Review drawing.** Each redraw restores the transcript band from `review.frm`. It copies 357 rows of 640 bytes starting at pixel (113, 76), so rows 77-432 are restored in full, row 76 only from x = 113, and row 433 only up to x = 112. Entries are then drawn from the top entry onward. With the line step s = text height + 2 = 12 and the current line y starting at 76, each entry is drawn as follows.

1. The target's name followed by `:` is drawn at (88, y) in key 992 [215], with no background fill and a drawing limit of 180 pixels. The text area's top moves down by 12.
2. The reply text is wrapped into the area x 113-422 (width 309) from the current top, in key 768 [198]. Its first line is drawn at x = 123, and later lines at x = 113, 10 pixels apart. Afterwards y is the next free line.
3. If an option was chosen after this reply, the player's name followed by `:` is drawn at (88, y) in key 21140 [6], the top moves down by 12, and the option text is wrapped the same way in key 15855 [9].
4. Drawing stops after the first entry that leaves y at 407 or more.

The window then refreshes (88, 76) through (434, 432). A line is drawn only while its top is at most 408, so the last entry may be cut, and a name can be left at the bottom with no text after it. `[Must-match]` The row-76 pixels at x 88-112 are not restored, so the antialiased top row of the first name keeps blending over its earlier copy. `[May-improve]`: restore the rectangle (88, 76) through (434, 433) before drawing.

**Review text sources** (SPEC-006 §3.9 records them):

- A message-backed reply or option is looked up again at every redraw, from its message list and number.
- A literal reply is shown as stored. A literal option is stored with its displayed `95 20` bullet prefix, so it shows that bullet, while a message-backed option shows the bare message text.
- The continuation option (-1/-1) shows an empty text after the player's name, and the Done option (-2/-2) shows `proto.msg` 650 "[Done]".
- A message that does not exist shows `Error`.
- A message list that cannot be loaded makes the reference stop the program with a system error. `[May-improve]`: draw `Error` instead.

**Review wrapping** (also used by the dialogue reply area):

1. If the rest of the text is at most the width W wide, it is one line.
2. Otherwise the line runs to the first space after the line's first character. It is then extended to each later space while the text up to that space stays narrower than W. The space at the break is dropped.
3. If the rest contains no space after its first character, the whole rest is drawn as one line, cut at W by the font's drawing limit, and the text ends there.
4. When the chosen line is still wider than W (a first word wider than W), drawing stops for that text. `[Must-match]` In that case the reference leaves the message's stored text cut at that word for the rest of the session. `[May-improve]`: never modify the stored message text.

**Closing Review.** The head updater is resumed and the previous font is restored. The window is removed, and the reply/choice loop continues. Stopped speech is not restarted.

**Tell Me About.** When pressed, or on `a`, the target is tested in this order.

1. **Not a critter.** Nothing happens. `[Must-match]`
2. **Reaction level bad.** The NPC's numeric reaction (SPEC-006 §3.10, initialized on first use) is at most 25: the reference looks up `proto.msg` 904 but shows nothing. `[May-improve]`: show `proto.msg` 904 in the reply area, as for map 35.
3. **Map number 35** (`V13ENT`): `proto.msg` 904 "This person has nothing to tell you about." is shown in the reply area, and the loop continues.
4. **Otherwise:**
   - the reply and option windows are hidden;
   - on the next head-updater tick, the Ask About window opens from the reply/choice loop's next input pass.

   The reference intends to stop speech first, but its test restarts or continues the speech buffer instead, so speech and lip-sync go on while the window is open. `[May-improve]`: stop playing speech, as Barter and Review do.

**Ask About window.**

- **Window.** A modal window at screen `(trunc((640 - 290) / 2), 356)` = (175, 356), 290 by 85. Its background is `lsgbox.frm` (238, 290 by 85), copied opaquely.
- **Text.** Drawn in font 103, key 18979 [62]:
  - `misc.msg` 6000 "Ask about what?" centered at y = 7 (x = 91 in the corpus font);
  - `dbox.msg` 100 "DONE" at (56, 57);
  - `dbox.msg` 103 "CANCEL" at (181, 57).
- **Buttons.** Two Red buttons, `lilredup.frm` (8) / `lilreddn.frm` (9), 15 by 16, drawn transparently, with the Red-button sounds:
  - at (34, 58), sending Enter;
  - at (160, 58), sending Esc.
- **Input field.** (22, 32) 244 by 14. Each redraw fills it with key 0 [228] and draws the typed text followed by the cursor character in font 101, key 992 [215], with a 244-pixel drawing limit. The view would scroll by dropping leading characters while the text is wider than 244, but the entry rule below never lets that happen.
- **Cursor.** Starts as `_`. On every input poll where more than 333 milliseconds have passed since the last toggle, it alternates between `_` and a space, and the field is redrawn.
- **Saved reply.** The displayed reply text is saved when the window opens.

**Ask About input.** Each poll reads one input code.

- **Backspace** removes the last character.
- **Enter**, or the DONE button, submits the text (SPEC-006 §3.9 Ask About lookup). The field is then cleared, whether or not anything matched, and the window stays open. Submitting an empty field does nothing.
- **Esc**, or the CANCEL button, stops any playing speech and closes the window.
- **Any other code of 0 or more** is appended when fewer than 126 characters are typed, and when the width of the typed text plus `_` plus the new character's glyph width (without spacing) is below 244.
- No quit key is handled inside the window.

The code is stored as one byte. Extended keys therefore enter characters: Up arrow (328) enters `H`, and F1 (315) enters `;`. Their widths are looked up outside the 256-glyph table. `[May-improve]`: accept only codes 32 through 255.

**Closing Ask About.** The window is removed and the previous font restored. The saved reply text is put back, the reply and option windows are shown again, and the reply is redrawn from the current reply page offset. Ask About replies are not added to the review history. An Ask About reply that pages, or a reply drawn while the original stood on a later page, uses the shared reply page offset: a long Ask About reply leaves that offset pointing into its own text. The restored reply then starts at a wrong position, and an Ask About reply shown while the original was on page 2 or later starts from that offset in the new text, possibly past its end. `[May-improve]`: draw each Ask About reply from its start and restore the original page offset on closing.

### 3.46 Floating Text

Floating text is short text drawn in the world over a map object. In R1 it has two users: the `float_msg` opcode (`0x810A`, SPEC-005 §3.5) and combat taunts (SPEC-009 §3.17). Look-at descriptions, combat messages and other notices go to the message monitor (section 3.20), not to floating text. `[Must-match]` unless tagged.

**Creation.** Each text has an owner object, a string, a font, a text color and an outline color (or no outline). Every R1 text has an owner.

| User | Font | Text color | Outline |
| --- | --- | --- | --- |
| `float_msg` style -2 | 103 | key 31744 [133] | key 0 [228] |
| `float_msg`, every other style | 101 | Per style, below | key 0 [228] |
| Combat taunt | The packet's `font`; 101 in every corpus packet | The packet's `color`, a raw index with the drawing flags of SPEC-002 §3.1; raw index 58 in every corpus packet | The packet's `outline_color` as a raw index, or none for -1; raw index 55 in every corpus packet |

`float_msg` style colors (SPEC-005 §3.5), with the palette index the corpus palette returns:

| Style | Color |
| --- | --- |
| 0, 8, and any unrecognized style | key 32747 [57] |
| 1, 5, 10 | key 10570 [11] |
| 2 | key 31744 [133] |
| 3 | key 992 [215] |
| 4 | key 31 [106] |
| 6 | key 21140 [6] |
| 7 | key 32074 [131] |
| 9 | key 32767 [220] |
| 11 | key 8456 [208] |
| 12 | key 15855 [9] |

- Style -1 takes the next style of the shared cycle 2, 3, ... 12, 1, 2, ..., starting at 2 (SPEC-005 §3.5). The cycle position is kept for the whole program run. A new game, a load or a map change does not reset it.
- Style -2 recenters the view on the player before the text is placed.
- `float_msg` requires its object to be on the current elevation (SPEC-005). A taunt has no elevation test.

A creation is refused, and nothing appears, in three cases. An earlier text of the same owner then stays.

- The string is empty.
- 19 texts already exist. The table has 20 slots, but a creation is refused once 19 are in use. Texts waiting for removal still count.
- The wrap below fails, because the text needs more than 63 lines.

**Layout.**

1. **Wrap.** The string is wrapped with the shared word wrapper of section 3.41.3 at width 200, in the text's font. A string narrower than 200 pixels is one line. A line feed or other control byte is not a forced break: it is a break point like a space, and it is drawn as a glyph.
2. **Trailing characters.** A line that ends at a break after a space loses that space. A line that ends after a hyphen keeps the hyphen. The last line of a text of two or more lines keeps a trailing space, but a one-line text loses it.
3. **Box.** The box width W is the widest line's width (SPEC-002 §3.1, final letter spacing included). The box height H is the line count times (font line height + 1): 11 per line for font 101 and 21 for font 103. With an outline, 2 is added to both W and H.
4. **Lines.** The box starts as index 0, which is transparent. Line k, counted from 0, is drawn with its left edge at x = `trunc((W - w) / 2)`, where w is the line's width, and its top at y = `k × (line height + 1)`, plus 1 with an outline. Glyphs are drawn as in SPEC-002 §3.2 over index 0. Partial coverage levels therefore blend toward palette index 0, and a pixel whose result is index 0 stays transparent.
5. **Outline.** With outline index c, two passes paint the outline into the box:
   - **Row pass.** Rows 1 to H - 2 are scanned left to right, each starting "outside". At a nonzero pixel while outside, the pixel to its left is painted c and the scan goes inside. At a zero pixel while inside, that pixel is painted c and the scan goes outside.
   - **Column pass.** Every column is scanned, rows 0 to H - 1, top to bottom, each starting outside. At a nonzero pixel while outside, the pixel above it is painted c. At a zero pixel while inside, that pixel is painted c. The scan switches as in the row pass.
   - Pixels painted by the row pass count as nonzero in the column pass, so the outline also covers the diagonal corners. Column 0 and row 0 are always blank before the passes, so no paint falls outside the box.
   - Outline index 0 paints transparent pixels. There is then no visible outline, but the box keeps its extra 2 pixels.

**Placement.** The position is chosen once, at creation. Let (hx, hy) be the window position of the owner's hex: the top-left of its 32-by-16 cell, as in section 3.27, in the 640-by-380 map window. The candidate top-left corners below are tried in order. The first one whose whole box lies inside the window is used, that is x ≥ 0, x + W ≤ 640, y ≥ 0 and y + H ≤ 380.

1. (hx + 16 - `trunc(W / 2)`, hy - H - 60)
2. Candidate 1 moved left by `trunc(W / 2)`
3. Candidate 2 moved right by W
4. (hx - 16 - W, hy - 16 - H)
5. (hx + 48, hy - 16 - H)
6. (hx + 16 - `trunc(W / 2)`, hy)
7. Candidate 6 moved left by `trunc(W / 2)`
8. Candidate 7 moved right by W

If no candidate fits, candidate 1 is used anyway and the box is clipped. The 60-pixel rise is fixed: the owner's art height plays no part.

The chosen offset from the hex is kept. Each draw places the box at the hex's current window position plus that offset, so the text scrolls with the map, and later scrolling can move it partly or wholly out of the window. The text is anchored to the hex the owner stood on at creation, not to the owner. When the owner walks away, the text stays behind. `[Must-match]`

**Drawing.** Floating text is the frame layer after the outlines and before the objects that have no hex (section 3.44). Every text is drawn in creation order, transparently (index 0 is skipped), and clipped to the refreshed rectangle. It is not hidden by roofs, and it does not depend on whether its owner is visible. A creation refreshes the new box's rectangle. A removal refreshes the union of the removed boxes, at their current positions. `[Must-match]`

**Lifetime.** `[Must-match]`

- **Removal time.** A text is removed at the first background update in which strictly more than `base + lines × line` milliseconds have passed since its creation. Background updates run on every input poll while background processing is on.
- **Base and line delays.** base = `trunc(text_base_delay × 1000)`. line = `trunc(min(max((text_base_delay - 1) × 0.4, 0), 2) × 1000)`. With the default 3.5, a text lasts 3500 milliseconds plus 1000 per line. The range is 1000 milliseconds flat at a delay of 1.0, up to 6000 plus 2000 per line at 6.0.
- **The line-delay key.** `fallout.cfg` [preferences] `text_line_delay` is read at start-up. Before any text can appear, the preference application of section 3.18 replaces it with the value derived from `text_base_delay`, and the derived value is what is written back. The 1.399994 default therefore never times a text.
- **Creation time.** The creation time is the time of the latest background update, not the exact moment of creation.
- **Real time.** The clock is real time. Removal is not checked while map background processing is off, such as during the action menu (section 3.28), the Pip-Boy, inventory, Skilldex, options, automap, character screen, elevator panel, world map, dialogue and map loading, or while the game is paused. The clock keeps running meanwhile. A text whose time passed is removed at the first update afterwards: its lifetime is not extended.
- **Not during rolls.** No background update runs during a counter roll (section 3.21).

**Replacement and removal.** `[Must-match]` unless tagged.

- A new text marks every existing text of the same owner for removal. Marked texts go at the next background update, so the new text replaces the old. Until then a marked text still counts toward the limit and is still drawn.
- Starting a dialogue removes every text at once.
- Every game reset removes every text: a new game, loading a save, and returning to the main menu.
- Nothing else removes a text. A text survives its owner's death, destruction or hiding, the start and end of combat, map changes and elevation changes.
- A text still alive when a new map loads or the elevation changes is drawn on the new map or elevation, at the same hex number and offset, until it expires. `[May-improve]`. Safe rule: remove every floating text when a map is loaded and when the elevation changes.
- After its owner is destroyed, R1 still matches the text against new owners for replacement, so a later object may be taken for the destroyed one. `[May-improve]`. Safe rule: a destroyed owner's text keeps its lifetime but matches no new owner.

**Gating.** No preference hides floating text. `float_msg` is not gated by `combat_messages` or `combat_taunts`, and it works in and out of combat. Taunts are gated by `combat_taunts`. A taunt is also skipped while any floating text exists anywhere, including texts waiting for removal and stale texts from another map or elevation (SPEC-009 §3.17). `[Must-match]`

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

### 4.8 Fixture H: Called-Shot Window

Use the shooter, weapon and target of SPEC-007 Fixture C: a right primary single-fire rifle in the right-primary-aimed slot mode, player AP 8, in combat. The target is a man-category critter whose current art base index is 13 (`hmmaxx`, alias 11). Every hit-chance term except the location adjustment sums to 46: skill 80, range -12, cover -10, AC -12.

| Row | Left part: chance, cells drawn | Right part: chance, cells drawn |
| ---: | --- | --- |
| 0 | Head 6: source x 0 at (33, 36), 54 at (42, 36) | Torso 46: source x 36 at (373, 36), 54 at (382, 36) |
| 1 | Eyes -14: minus (source x 108, 6 wide) at (33, 102) and (42, 102) | Groin 16: source x 9 at (373, 102), 54 at (382, 102) |
| 2 | Right arm 16 at (33, 166) and (42, 166) | Left arm 16 at (373, 166) and (382, 166) |
| 3 | Right leg 26 at (33, 230) and (42, 230) | Left leg 26 at (373, 230) and (382, 230) |

| Input | Expected |
| --- | --- |
| Labels in row 0 | `combat.msg` 1110 "the head" at (74, 36), button (33, 40) 88 by 10; 1113 "the torso", 60 wide in font 101, at (291, 36), button (291, 40) 88 by 10 |
| Label in row 3, right | 1115 "the left leg", 71 wide, at (280, 230), button (280, 234) |
| Left release at window (40, 236) | Row 3 left button: right leg, location 4, chance 26 as in Fixture C; `icsxxxx1`; debit `5 + 1 = 6` AP |
| Left release at window (125, 236) | Outside the button (x 33-120), although "the right leg" (78 wide) is drawn through x = 151: nothing |
| Ctrl+D | Code 4: torso, location 3, chance 46, still debited 6 AP |
| Key `4` | Nothing |
| Esc | Window closes; AP 8, no draw, no sound |
| A melee weapon whose other terms also sum to 46 (half adjustments) | Head 26, eyes 16, arms and groin 31, legs 36, torso 46 |
| Other terms sum to 120, ranged | Torso 95 (cap), head 80, eyes 60, arms and groin 90, legs 95 |
| Target base index 15 (`mabrom`), row 3 right | 1155 "the left foreleg", 97 wide, at (254, 230), button (254, 234) |

### 4.9 Fixture I: World Map and Town Map

**World view.** The party is at the starting position (825, 75), Vault 13's cell (16, 1), with follow mode on.

| Input | Expected |
| --- | --- |
| Centering | vx = 825 - 247 = 578; vy = 75 - 242 = -167, clamped to 0. The party is at window (269, 96) |
| At-rest triangle | (257, 90), 25 by 13; tip at (269, 102) |
| Visible cells | Columns 11-20 and rows 0-8. Column 11 is 22 wide at x 22-43, columns 12-19 are at x `44 + 50 (c - 12)`, and column 20 is 28 wide at x 444-471. Rows 0-7 are at y `21 + 50 r`; row 8 is 42 tall at y 421-462 |
| Discovery after the opening reveal (no Scout) | Cell (16, 1) at (244, 71) through (293, 120) is unchanged. The other eight cells of columns 15-17, rows 0-2 are dimmed, and every other visible cell is black (key 0 [228]). Row 1's ocean threshold is 0, so nothing else is revealed |
| Hover with pointer (279, 95) | dx = 10, dy = 0, and dy is halved to 0: the sum is 10, so the pointer hovers |
| Hover with pointer (279, 100) | dx = 10, dy = 5, and dy is halved to 2: the sum is 12, so it does not hover |
| Hover with pointer (273, 103) | dx = 4, dy = 8, and dx is halved to 2: the sum is 10, so it hovers |
| Hover label "Vault 13" (font 101, width 50) | x = 257 + trunc(-25 / 2) = 245, y = 79; the label fills (245, 79) through (294, 88) |
| Destination click at pointer (400, 300) | Destination (956, 279), cell (19, 5): no town, so no jitter. `ib1p1xx1`; follow mode stays on |
| Travel dots | Iterations 1, 4, 7, 10 mark a pixel; iterations 2, 3, 5, 6, 8 and 9 do not |
| Pointer at (639, 240), vx = 950 | East is blocked: `screx.frm`, and the view stays put |
| Pointer at (639, 0), vx = 900, vy = 0 | Only north is blocked: the normal north-east art, and the view moves to (916, 0) |
| Pointer at (639, 0), vx = 950, vy = 0 | Both axes are blocked: `scrnex.frm` |
| Home | Centers to (578, 0), then sets vy = 0 and steps 16: (578, 16). Under the safe rule: (578, 0) |
| End at vx = 578 | (578, 1058) |
| Clock tick 264600 | Day digits 0 and 5 at x 487 and 496; month rows 165-178 (DEC) at (513, 12); year digits 2, 1, 6, 1 at x 548, 557, 566 and 575; time digits 0, 7, 2, 1 at x 593, 602, 611 and 620 |
| Seen-cell pixel indices 50, 150, 100 (corpus palette, generated intensity table) | 52, 153, 228 |
| Ordinary encounter at party window position (269, 96) | `wmapfgt0.frm` / `wmapfgt1.frm` alternate 7 times at (266, 91), 199 milliseconds each, 2786 milliseconds in all |

**Town map.** All label widths are in font 101.

| Input | Expected |
| --- | --- |
| Hub, slots 0, 1 and 3 known | Three markers. Event 514 is slot 0, `HUBENT.MAP` (36) with selector 1; event 515 is slot 1, `HUBDWNTN.MAP` (38) with selector 3; event 516 is slot 3, `HUBOLDTN.MAP` (40) with selector 2 |
| Hub slot 3 geometry | Button (249, 131), 164 by 25; triangle at (318, 143); the hover label "Old Town" (width 52) is at x = 249 + 56 = 305, y = 131 |
| Pointer at (260, 140) over the Hub slot 3 button | Outside the triangle's pixels: no hover, no label, no click |
| Vault 13 at tick 863999 / 864000 | One marker, "Cavern Entrance" (`V13ENT.MAP`, 35, selector 0) / four markers |
| Cathedral with campaign index 18 = 1 | One marker at triangle (247, 307), label "Crater" (width 40) at x = 178 + 62 = 240. Choosing it loads `CHILDEAD.MAP` (47) with selector 0 |
| Military Base with index 17 = 1 | One marker at triangle (118, 70), label "Ruins"; it loads `MBDEAD.MAP` (48) with selector 0 |
| Vault 15 slot 0, party in Vault 15's cell | `VAULTENT.MAP` (7) loads with index 32 = 0 |
| Vault 15 slot 2, party at Vault 13 | The world map opens with the party travelling to Vault 15's jittered center. It stops there at rest, and `VAULTBUR.MAP` does not load until the player enters again |

### 4.10 Fixture J: Pip-Boy Screen

Font 101 widths from the corpus `font1.aaf`: STATUS 47, DATA 33, BACK 32, MORE 31, DONE 31, AUTOMAPS 65, VIDEO ARCHIVES 99, ALARM CLOCK 83, "of" 13, "Hit Points 25/30" 103, "Vault 13" 50, "Hub" 22, "FEV Experiment Tape" 127, "Merry Christmas!" 101, `pipboy.msg` 215 209.

| Input | Expected |
| --- | --- |
| Game tick 264600 (07:21, 5 December 2161) | Day cells 0, 5 at (20, 17), (29, 17); `months.frm` rows 165-178 at (46, 18); year cells 2, 1, 6, 1 at x = 83, 92, 101, 110; time cells 0, 7, 2, 1 at x = 155, 164, 173, 182; all at y = 17 |
| 25 December, normal opening | `pipx.frm` at (323, 156); "Merry Christmas!" at (381, 315) |
| Global 10 = 150, global 101 = 0 | `pip2.frm` at (32, 83); note digits 0 at (92, 180), 5 at (80, 182), 1 at (68, 184) |
| Global 10 = 11 | Digits 1 at (92, 180), 1 at (86, 182) |
| Global 101 = 2, or global 10 = 0 | Note rectangle restored from `pip.frm` |
| Status, only the water-chip quest, no holodisks | STATUS at (405, 46), underlined; "Vault 13" at (262, 66); 2 zones, (254, 66) and (254, 86), 350 by 10 |
| Release at (300, 70) | `ib1p1xx1`; "Vault 13" yellow for 200 ms; then "Vault 13 Status" at (262, 56), underlined; "1. Find the Water Chip." at (262, 76); BACK at (413, 446) |
| Same, global 101 = 2 | Line in key 8804 [218], strike line y = 79 from x = 262 through 396 |
| Release at (300, 90) on that list | Sound and 200 ms pause; nothing else; the Vault 13 row still opens its detail; the Status button resets |
| Global 162 = 1 (holodisk 0) | STATUS at (317, 46); DATA at (489, 46); "FEV Experiment Tape" at (437, 66) |
| Release at (500, 70) | Reading view, page 0 of 3 (94 records, last-page index 2): title at (365, 46), underlined; "1 of 3" at (567, 48); records 1000-1034 at x = 255, y = 76 through 416; BACK at (332, 446); MORE at (491, 446) |
| Release at (500, 450), twice | Page 1 shows records 1035-1069, counter "2 of 3"; page 2 shows records 1070-1093 and DONE |
| On page 2: PageDown; release at (420, 450); release at (380, 450) | Nothing; nothing; page 1 |
| Automap database holds map 6 elevations 0 and 1, map 35 elevation 0, map 36 elevation 0, map 3 elevation 0 | Area list: Hub (262, 66), Necropolis (262, 86), Vault 13 (262, 106) |
| Row 1 (Hub) | Line 2 "Hub" (`map.msg` 136) at (418, 66); entries "Entrance" (`map.msg` 308, yellow) at (262, 86) and, in R1, "Sewers" (`map.msg` 209, map 3) at (262, 106); map 36 elevation 0 raster drawn. Safe rule: "Entrance" only |
| Row 3 (Vault 13) | Line 2 "Vault 13" (`map.msg` 106); "Entrance", "Living Quarters" (218, 219). Safe rule adds "Cave Entrance" (305, map 35) |
| Elevation view with 2 entries | Zones at y = 46, 66, 86, 106 report rows -1, 0, 1, 2 |
| Archives with index 3 and 5 played | "Vat Destruction" (262, 66), "Overseer Briefing" (262, 86); row 2 plays `ovrintro.mve` with flags `0x0B` |
| Alarm Clock, HP 25 of 30 | ALARM CLOCK at (387, 46); "Hit Points 25/30" at (377, 66); option j at (262, 96 + 20j); "Rest until healed" at (262, 336) |
| 07:21, until morning / until noon | 22 h 39 min / 4 h 39 min |
| 06:00, until morning / 12:00, until morning / 06:30, until morning | 24 h / 18 h / 23 h 30 min |
| Status list with at least 4 zones; Alarm Clock refused; then a release on the 4th zone (y = 126) | R1: ten-minute rest begins on the Alarm Clock page; safe rule: the Status row's own meaning |
| Screen saver draws 100, 16383, 16383 | Spawn: P = `trunc(16383 / 32767 × 724)` = 361, start (602, 409); speed 1.5249580 in single precision; the same pass clips it to 2 by 43 and moves it to (600, 410) |
| Draws 100, 20000, 0 | P = 441: start (317, -17); speed 0.15; seven passes reach progress 1.05, moving to (315, -15) |
| Draw 3048 | No spawn; one draw this pass |

### 4.11 Fixture K: Level Block and Grouped Numbers

| Input | Expected |
| --- | --- |
| Level 6, experience 15500 | y 280 `Level: 6`; y 291 `Exp: 15,500`; y 302 `Next Level: 21,000` |
| Level 21, experience 215000 | `Level: 21`, `Exp: 215,000`, `Next Level: ------` |
| Grouping of 0, 999, 1000, 100000, 1234567, 12345678 | `0`, `999`, `1,000`, `100,000`, `1,234,567`, `12,345,678` |
| Unspent skill points 101 (Educated after the 99 cap) | Big number at (522, 228) shows `99` |

### 4.12 Fixture L: Hit Tests

| Press (screen x, y) | Folder | Info line | Card |
| --- | --- | --- | --- |
| (60, 290) | Any | 8 | Experience |
| (60, 311) | Perks, no perks held | 10 | Perks (`editor.msg` 124 / 127); the unclamped level-block defect |
| (100, 380) | Kills, 12 types killed | 11 | Second kill type in name order |
| (200, 380) | Kills, 12 types killed | 21 | Twelfth kill type (right column, row 1) |
| (100, 455) | Perks, 2 perks, 2 traits | 18 (folder line 8) | First trait slot |
| (200, 340) | Any (tab press and release) | 42 | Karma general card; `ib3p1xx1` |
| (250, 147) | Any | 51 | Armor Class; the unclamped conditions defect |

### 4.13 Fixture M: Skill Spending

Start with Small Guns at 55%, untagged, 5 invested points, 2 unspent points, and the skill cursor on Small Guns.

1. Plus: 56%, 1 unspent; the big number animates 02 to 01.
2. Plus: 57%, 0 unspent.
3. Plus: `iisxxxx1`, box "No free skill points available."; the hold ends.
4. Minus: 56%, 1 unspent.
5. Minus: 55%, 2 unspent.
6. Minus: 55% equals the value at open, so the box titled "Small Guns:" shows "At minimum level." / "Unable to decrement it."
7. Cancel: the snapshot restores 55%, 5 invested points and 2 unspent points.

### 4.14 Fixture N: Perk Chooser Selection

There are 14 eligible perks, so the maximum top is 3. Start at top 0, row 0.

| Input | Top, row | Selected entry |
| --- | --- | --- |
| Down x3 | 3, 0 | 3 |
| Down x2 | 3, 2 | 5 |
| Up | 2, 2 | 4 |
| End | 3, 10 | 13 |
| Home | 0, 0 | 0 |
| Click at screen y = 170, then again at y = 175 before the idle timer fires | 0, 3 | 3, chosen with `ib1p1xx1` |

### 4.15 Fixture O: File Dialog Paging

There are 14 files, F00 through F13, so the list scrolls and M = 14 - 13 = 1. Start at top 0, row 0.

| Input | Load dialog (top, row, entry) | Save dialog (top, row, entry) |
| --- | --- | --- |
| Down key | 1, 0, F01 | 1, 1, F02 (the press that reaches M also moves the row) |
| Down key x11 more | 1, 11, F12 | 1, 11, F12 |
| Down key | 1, 12, F13 (row 12 is not drawn) | 1, 11, F12 |
| Visible rows | F01-F12; F13 never drawn | Same |

With the safe rule, M = 2 and F13 is drawn as the last row of top 2.

### 4.16 Fixture P: Printed Sheet

The sheet is for a synthetic player:

- name Alex, age 25, male; level 6, experience 15500;
- ST 6, PE 7, EN 5, CH 4, IN 8, AG 7, LK 3; HP 47/51, AC 7, AP 8, healing rate 1, critical chance 3, melee damage 1, carry weight 175;
- damage resistance 0, radiation resistance 10, poison resistance 25, sequence 18;
- traits Bloody Mess and Fast Shot; perk Earlier Sequence rank 2;
- inventory 3 Stimpak, 1 10mm Pistol, 24 10mm JHP, 1 Leather Jacket, total weight 18;
- kills 5 Rats, 2 Radscorpions; skills Small Guns 55%, Big Guns 10%, Energy Weapons 10% (the remaining skills are omitted from the safe-rule excerpt);
- game date 12 December 2161, 09:30.

The reference output with the English corpus `editor.msg` follows. Line numbers and `|` delimiters are not part of the file. Every line ends in CR LF, and trailing spaces are shown inside the delimiters.

```text
 1||
 2||
 3|                                  FALLOUT|
 4|                         VAULT-13 PERSONNEL RECORD|
 5|                        12 December 2161  0930 hours|
 6||
 7|  Name: Alex               Age: 25               Gender: Male|
 8| Level: 06                 Exp: 15,500       Next Level: 21,000|
 9||
10|       Strength: 06         Hit Points: 047/051         Sequence: 06|
11|     Perception: 07        Armor Class: 007         Healing Rate: 01|
12|      Endurance: 05      Action Points: 08       Critical Chance: 003%|
13|       Charisma: 04       Melee Damage: 01          Carry Weight: 175 lbs.|
14|   Intelligence: 08        Damage Res.: 000%|
15|        Agility: 07     Radiation Res.: 010%|
16|           Luck: 03        Poison Res.: 025%|
17||
18||
19| ::: Traits :::           ::: Perks :::           ::: Karma :::|
20|  Bloody Mess|
21|  Fast Shot|
22| ::: Skills :::                ::: Kills :::|
23|  Earlier Sequence (2)|
24||
25| ::: Inventory :::|
26||
27|                                      Total Weight:|
28||
29||
30|Error|
31|  3x Stimpak               1x 10mm Pistol           24x 10mm JHP           |
32|  1x Leather Jacket      |
33||
34|Error 18 lbs.|
35||
36||
```

Line 10 shows Sequence 06, which is the Strength defect. Line 22 is the misnumbered perks heading. With all four safe rules, lines 1-18 are the same except that line 10 ends in `Sequence: 18`. The rest of the file becomes the following. Only the first three skill rows are shown; the real file has 18, so every later line moves down by 15.

```text
19| ::: Traits :::           ::: Perks :::           ::: Karma :::|
20|  Bloody Mess|
21|  Fast Shot|
22|  Earlier Sequence (2)|
23||
24||
25| ::: Skills :::                ::: Kills :::|
26|  Small Guns ..... 055%        Radscorpions ... 002|
27|  Big Guns ....... 010%        Rats ........... 005|
28|  Energy Weapons . 010%|
29||
30||
31| ::: Inventory :::|
32|  3x Stimpak               1x 10mm Pistol           24x 10mm JHP           |
33|  1x Leather Jacket      |
34||
35|                                      Total Weight: 18 lbs.|
36||
37||
```

Pairing follows kill-type order, not the name order of the Kills folder, so Radscorpions (type 6) pairs with Small Guns and Rats (type 7) with Big Guns.

### 4.17 Fixture Q: Object Draw Order and Translucency

On elevation 0, hex 20100 holds, in load order: flat item A, flat item B, critter C, scenery D. Hex 20101 holds flat item E and wall F. Nothing is hidden, and the whole screen is refreshed.

| Step | List of hex 20100 | Draw order |
| --- | --- | --- |
| After load | A, B, C, D | A, B, E, then C, D, F |
| C takes one walk step on hex 20100 (a new pixel offset) | A, B, D, C | A, B, E, then D, C, F |
| C steps onto hex 20101 | A, B, D; hex 20101 is E, F, C | A, B, E, then D, F, C |

Translucency, with source 5-bit components (7, 31, 0): the common gray level is `trunc(trunc((0 + 21 + 186) / 10) / 4)` = 5, and the glass gray level is `trunc(trunc((7 + 155 + 0) / 10) / 4)` = 4. A wall-translucent pixel of that color over destination D therefore draws blend_F[5][D], whose components are `trunc((5 F + 2 D) / 7)`, at intensity column 128. A source pixel whose gray level is 0, such as (0, 1, 0), draws intensity[D][L].

With the corpus `color.pal`, the five foreground colors are indexes 33 (wall), 35 (glass), 220 (steam), 58 (energy) and 133 (red). Source 215 over destination 100 draws 6 as wall translucency, and 214 as glass translucency at full light.

### 4.18 Fixture R: Dialogue Review and Ask About

Font 101 widths from the corpus `font1.aaf`: "Abel:" 29, `_` 8, "Water" 35. Font 103 (`font3.aaf`): "Ask about what?" 107.

| Input | Expected |
| --- | --- |
| Dialogue panel | Window (0, 290) 640 by 190; Review at screen (13, 444), Barter at (593, 331), Tell Me About at (593, 406) |
| Review, entries: reply "Hng." (one line) with an option chosen, target named Abel | "Abel:" at (88, 76) key 992; "Hng." at (123, 88) key 768; player name and `:` at (88, 98) key 21140; option first line at (123, 110) key 15855; the next entry's name at (88, 120) |
| A reply needing two lines | First line at x = 123, second at (113, top + 10) |
| 5 entries, Down pressed 6 times | Top entry 1, 2, 3, 4 (each redrawn), then 4 twice with no redraw |
| Up at entry 0 | No redraw |
| Ask About window | Screen (175, 356); title at (91, 7); DONE at (56, 57); CANCEL at (181, 57); Enter button (34, 58); Esc button (160, 58); field (22, 32) 244 by 14 |
| Target NPC's reaction 20 | Pressing Tell Me About: button sounds only, nothing shown (safe rule: `proto.msg` 904) |
| Any critter on map 35 | `proto.msg` 904 in the reply area |
| On map 10, target script with no 1000-1099 keywords, submit "killian" | List 1 message 7000 "Killian" matches; reply = list 1 message 7100 "Killian is the mayor. He runs a store."; no random draw |
| Same, submit "water chip" | Tokens "water", "chip": no keyword matches (list 1 has "Water Chip" at 7005); no name match; fallback draw over the target's 980-999 |
| Target script `abel`, submit "Abel." | Token "Abel" equals the target's name: `abel.msg` has one non-`Error` message in 970-979, so `random(1, 1)` consumes one draw and shows 970 "Me Abel." |
| Target script `abel`, submit "xyz" | `abel.msg` has 980 and 981 in 980-999: one `random(1, 2)` draw; 1 shows "Hng.", 2 shows "Why do you say that?" |
| Typed text 230 pixels wide including `_`, next glyph 9 wide | 230 + 9 = 239 < 244: accepted |
| Typed text 236 pixels wide including `_`, next glyph 8 wide | 244 is not below 244: refused |
| Up arrow pressed in the field | Character `H` appended (safe rule: ignored) |

### 4.19 Fixture S: Dialog Box, Date Line, Screenshot and Key Repeat

| Input | Expected |
| --- | --- |
| Medium Done box at screen (200, 150) | `donebox.frm` at window (53, 81), screen (253, 231); DONE at window (88, 84); red button at window (66, 85), 15 by 16 |
| Medium size requested, title plus a second-button label | Forced large: Done box at (37, 98), second box at (169, 98) |
| `/` at tick 264600 | Monitor line "December: 5/2161 7:21" |
| `/` at tick 264600 + 23400 (39 minutes later) | "December: 5/2161 8:00" |
| `?` while the bar is disabled out of combat | Same line; `ib1p1xx1` |
| F12, folder holds `scr00000.bmp` and `scr00002.bmp` | Writes `scr00001.bmp`; monitor "Saved screenshot." |
| First 54 bytes of a 640-by-480 screenshot | `42 4D 36 B4 04 00 00 00 00 00 36 04 00 00 28 00 00 00 80 02 00 00 E0 01 00 00 01 00 08 00` followed by 24 zero bytes |
| Palette entry for 6-bit levels (63, 32, 0) | Bytes `00 80 FC 00` |
| Pixel at screen (0, 479) | File offset `0x0436`; screen (0, 0) is at `0x0436 + 479 × 640` = `0x4B1B6` |
| Tab held from t = 0 ms, polls every 41 ms | Press event at 0; first repeat at the poll at 533 ms (first with more than 500 elapsed); next at 615 (82 since the last repeat), then 697 |
| Subtitle entry 700 pixels wide whose last space before the 640-pixel point follows its 12th word | Only those 12 words are shown, centered; the rest is never shown. Safe rule: the rest as a second line at `y0 + th`, with the band grown to `2 th + 4` |

### 4.20 Fixture T: Grey Table, Outlines and Floor Light

Grey table over the corpus `color.pal` (section 3.17):

| Index i | 6-bit levels | s = max + min (5-bit) | v | Grey index |
| ---: | --- | ---: | ---: | ---: |
| 0 | unmapped | 0 | 0 | 228 |
| 1 | (59, 59, 59) | 58 | 27 | 2 |
| 57 | (63, 63, 31) | 46 | 21 | 5 |
| 145 | (63, 39, 18) | 40 | 18 | 7 |
| 215 | (15, 62, 0) | 31 | 14 | 9 |
| 220 | (63, 63, 63) | 62 | 29 | 1 |
| 229-255 | unmapped | 0 | 0 | 228 |

Outline ring of a 3-by-2 frame. Row 0 is (0, 5, 0) and row 1 is (5, 5, 5); local coordinates, nothing clipped (section 3.44):

| Pass | Paints, in order |
| --- | --- |
| Rows | (0, 0), (2, 0); then (-1, 1) and the end-of-row paint (3, 1) |
| Columns | (0, 0) and the end paint (0, 2); (1, -1), (1, 2); (2, 0), (2, 2) |
| Ring | 8 pixels: (1, -1), (0, 0), (2, 0), (-1, 1), (3, 1), (0, 2), (1, 2), (2, 2); (0, 0) and (2, 0) are painted twice |

| Input | Expected |
| --- | --- |
| That frame, type 8 friendly | Frame height 2 gives b = 0: every pixel is index 229 |
| That frame, type 2 with bit 30, destination index D | Each pixel is `blend_red[2][D]`; (0, 0) and (2, 0) become `blend_red[2][blend_red[2][D]]` |
| A 10-row frame, type 1 hostile | b = 2: rows 0-1 index 244, rows 2-3 245, rows 4-5 246, rows 6-7 247, rows 8-9 243 |
| Outline word `0x80000001` | Hostile, disabled: not collected for drawing |
| Outline word 3 | Not a listed type: key 31775 [49], opaque |
| Set type 16 on an object whose word is `0x80000008` | Refused: the word already has a type |

Floor light for an even base hex. All vertex lights are 65536 except V3 = 0, so the tile is interpolated:

| Tile pixel | Map entry | Value | Intensity column |
| --- | ---: | ---: | ---: |
| (32, 10), vertex 3 | 992 | 0 | 0 |
| (40, 10) | 1000 | 16384 | 32 |
| (48, 10) | 1008 | 32768 | 64 |
| (16, 4) | 496 | 49152 | 96 |
| (32, 0) | 192 | 55454 | 108 |
| (20, 20) | 1780 | 50410 | 98 |
| (0, 10), (64, 10), (48, 22) | | 65536 | 128 |
| (33, 35) | 2993, never written | 0 | 0 |

With all ten lights equal to 40000, the tile uses the lit draw at column `trunc(40000 / 512)` = 78, and indexes 229-255 stay unlit.

Per-hex order. A hex holds upright objects A then B, on the same elevation. B steps one animation frame whose shift is (0, 0): the order becomes A, B, unchanged because B was already last. A then steps one frame, giving B, A. A script then moves B to the hex it is already on, giving A, B, with B's pixel offset reset to (0, 0).

### 4.21 Fixture U: Floating Text

Font 101 widths are from the corpus `font1.aaf`: "Hello stranger. Welcome to" 160, "Shady Sands." 80, "Die, scum!" 58. Default preferences apply. Window positions are those of section 3.46.

| Input | Expected |
| --- | --- |
| `float_msg` style 0 "Hello stranger. Welcome to Shady Sands." (width 245); owner's hex at window (304, 200) | Two lines, "Hello stranger. Welcome to" and "Shady Sands."; box 162 by 24; line 0 at box (1, 1), line 1 at (41, 12); text key 32747 [57], outline key 0 [228]; candidate 1 fits: box at (239, 116), offset (-65, -84) |
| Same text; owner's hex at window (600, 50) | Candidates 1-3 fail (y = -34, then x + W = 778); candidate 4 fits: box at (422, 10), offset (-178, -40) |
| Same text, 5 seconds later | Still shown (lifetime 3500 + 2 × 1000 = 5500 ms); removed at the first background update after 5500 ms |
| The owner walks 3 hexes east after the text is created | The text stays over the creation hex |
| Combat taunt "Die, scum!" with packet font 101, color 58, outline 55 | Box 60 by 13; line at (1, 1); text raw index 58, outline raw index 55; lifetime 4500 ms |
| Taunt packet with `outline_color=-1` | Box 58 by 11; no outline |
| Box 5 by 3 whose only glyph pixel is (2, 1), outline c | Row pass paints (1, 1) and (3, 1); column pass paints (1, 0), (1, 2), (2, 0), (2, 2), (3, 0), (3, 2): a 3-by-3 ring around the glyph pixel |
| `float_msg` style -1 three times, first use in the session | Styles 2, 3, 4: key 31744 [133], key 992 [215], key 31 [106] |
| `float_msg` style 13 | Font 101, key 32747 [57] |
| `float_msg` style -2 "Warning" | Font 103, key 31744 [133]; the view recenters on the player first; box height 21 + 2 = 23 |
| `text_base_delay` 1.0, a 3-line text | 1000 ms (line delay 0) |
| `text_base_delay` 6.0, a 1-line text | 6000 + 2000 = 8000 ms |
| Owner A has a text; A floats a new one | The old text is removed at the next background update; the new one shows |
| 19 texts exist; any new `float_msg` | Nothing appears; an earlier text of the same owner stays |
| Any text exists; an NPC taunt fires | No taunt |
| A text is alive when the player takes an exit grid to another map | Drawn on the new map at the same hex number until it expires. Safe rule: removed on map load |

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings

Verification notes: `tools/spec_lint.py` gives 0 errors, and no source path or raw revision hash appears. Twelve public `fallout.cfg` key literals that coincide with reference identifiers were added to the linter allowlist (CLAUDE.md rule 2): `game_difficulty`, `violence_level`, `target_highlight`, `combat_messages`, `item_highlight`, `combat_speed`, `player_speedup`, `mouse_sensitivity`, `master_volume`, `music_volume`, `sndfx_volume` and `speech_volume`. The 33 plain-word warnings were reviewed and are benign: the configuration keys "splash", "brightness", "running" and "subtitles", and the configuration section name "system"; the sound name "monitor"; and formula words ("line", "time", "length", "width", "text", "current", "count", "trunc", and "column" in the section 3.36 roof-square formula). Campaign variables are cited by index only. The only fenced blocks are a synthetic hex dump and a credits text sample. The sound name "toggle" is also flagged. The section 3.29 border values were recomputed with an independent model of the hex-to-screen transform and checked against the entry hexes of all 72 corpus maps. Section 3.34 art sizes were read from the corpus FRM headers. Section 3.35 file and frame facts were checked against the corpus (`rscorpbl.frm` has 9 frames; item PID 213 is Remains of Gizmo). The section 3.27 busy-click rules were checked against SPEC-007 §3.10.1 ("One sequence per owner"). The section 3.36 empty-roof test and bits match SPEC-003 §2.8, and the roof words of all 72 corpus maps were scanned: 15056 roofed squares, none with roof index 0 or the barrier bit; `LAGUNRUN.MAP` stores 257 squares with the hide bit set, which loading clears. Fixture G was stepped by hand through the section 3.36 rule. Sections 3.38-3.43 (R1-rev8): every art name, list index and size was read from `intrface.lst` and the corpus FRM headers; `targbox.frm`, `wrldmp00.frm`-`wrldmp19.frm` and `pipboy.frm` do not exist in the corpus, and the real files are `called.frm` (118), `worldmap.frm` (135) and `pip.frm` (127). All 21 corpus `na` pictures are 170 by 225 with one frame. Every quoted `combat.msg`, `worldmap.msg`, `map.msg`, `pipboy.msg`, `editor.msg`, `proto.msg` and `dbox.msg` text was checked against the English corpus, and the fixture label widths were computed from `font1.aaf`. The town-map entrance table was checked against the map headers of every hotspot map, and the town pictures were composited with their markers. Fixtures H-P were stepped by hand or by a research-side model. The printed sheet of section 3.43 is documented literally: R1 reads `editor.msg` 650-655 with meanings that do not match the shipped file, so every resulting defect carries a `[May-improve]` safe rule. The section 3.27 combat-lock and section 3.23 hand-swap rules were checked against the sequence registration and keyboard paths. Section 3.44 (R1-rev9) was checked against the reference object renderer, list insertion and translucent blitters, and its foreground indexes and Fixture Q values were modelled over the corpus `color.pal`. The R1 tie-break inside a hex is incomplete in the reference and never decides, so the section states the insertion-order result. Tag count, excluding the two tag definitions: 228 `[Must-match]`, 102 `[May-improve]`.

Update notes (2026-10-23, R1-rev10): every new art name, list index and size in sections 3.45 and 3.44 was read from `intrface.lst` and the corpus FRM headers. The guessed `di_rview.frm` does not exist; the Review background is `review.frm` (102). Quoted `misc.msg`, `dbox.msg`, `proto.msg` and `editor.msg` texts were checked against the English corpus, and fixture widths were computed from `font1.aaf` and `font3.aaf`. The Ask About fixture keywords were read from the corpus dialog files. The screenshot header bytes and the grey table were modelled over the corpus `color.pal`. Net tags for R1-rev10: +17 `[Must-match]`, +12 `[May-improve]`. `tools/spec_lint.py` gives 0 errors.

Update notes (2026-10-23, R1-rev11): the input rules of sections 3.21, 3.25 and 3.27 were traced through the reference roll, panel, mouse and keyboard paths. The floating-text colors in section 3.46 were checked against the corpus `color.pal`, the taunt fonts and colors against all 95 `data/ai.txt` packets, and the fixture widths against `font1.aaf`. The placement window is the 640-by-380 map window. Net tags: +10 `[Must-match]`, +2 `[May-improve]`. `tools/spec_lint.py` gives 0 errors.

Update notes (2026-10-23, R1-rev12): the keyboard translation tables were checked for every modifier, lock and Windows key, and the cursor object flags were read from the game-mouse set-up. Net tags: +2 `[Must-match]`. `tools/spec_lint.py` gives 0 errors.
