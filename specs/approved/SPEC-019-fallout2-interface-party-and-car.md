# SPEC-019: Fallout 2 Interface, Party Control, the Car, and Game-Flow Deltas

**Status:** approved by the Spec-Author sub-agent (Team A), 2026-10-25, under AGENTS.md "Spec Authoring (Team A)". Revision 2 (retail art audit; scripts run) by the coordinator, 2026-10-25. Revision 3 (premade portraits) by the Spec-Author sub-agent (Team A), 2026-10-26, verified against the owner's retail install.

**Provenance:** written by Team A (the Spec-Author sub-agent) on 2026-10-25 from
the coordinator's brief of the same day. Source categories:

- the project's approved specifications: SPEC-001 to SPEC-016, and SPEC-018 §1;
  for the reconciliation pass of the same day, SPEC-017 and SPEC-018 in full;
- `docs/fallout2-and-qol-roadmap.md`, including part 2's enhanced companion
  features;
- Team A's general knowledge of publicly documented Fallout 2 interface and
  gameplay behavior: what a player sees and uses, and public SSL procedure
  and `ai.txt`/`party.txt` key names.

No decompilation, disassembly or re-implementation source was consulted, and
no game asset was read. Team A's general knowledge comes from model training
data whose own sources cannot be audited, so this specification cannot
certify that none of its content traces back to reverse-engineered material
(`PROVENANCE.md`, "In-session spec authoring (Team A)"). A later
research-vault revision of any section supersedes it.

**Revision 2 (retail corpus audit), 2026-10-25.** The coordinator wrote this
revision at the owner's request, from D-2's art report and from frame sizes
read from the owner's retail Fallout 2 install (the GOG release). It changes
three things:

- section 2.2 records the audit;
- section 3.15.3's custom panel takes its art's size;
- section 3.4's gated session no longer applies, because SPEC-017 Revision 3
  opens the script gate.

No game art is reproduced.

**Revision 3 (premade portraits), 2026-10-26.** By the Spec-Author sub-agent
(Team A), 2026-10-26, verified against the owner's retail install.

- **Origin.** The owner reported the premade selection screen drawing a
  portrait over the monitor's top-left bezel. The coordinator drafted this
  revision with the fix. Team A then re-measured it independently and took
  it over.
- **Method.** Team A's own throwaway scripts were written from SPEC-015
  (archives) and SPEC-001 (FRM). They read the frame sizes and compared the
  portraits' border pixels with `pickchar.frm`'s. Team A did not read the
  engine's code. The pixels were read locally and are not reproduced.

The retail portraits are 592 by 260, not SPEC-014's 212 by 187, so
SPEC-014's placement formula puts them at (-146, -20), mostly off the
monitor. The revision changes two things:

- section 2.2 records the sizes of the selection screen's art, and how the
  fit was measured;
- section 3.3 places the portrait in the monitor window and clips it there
  (decision D-32).

Team A's clarifications to the coordinator's draft:

- **The fit.** It is stated with its numbers, as decisive about the offset
  but not an exact identity.
- **The tags.** The placement is a measured `[Team A default]` and the clip
  a `[Safe rule]`.
- **Fallout 1.** It is stated to be unchanged.
- **Vector R.** It now names every point it checks, including Fallout 1's.

The sheets' name, tags and traits are moved by Fallout 2's longer sheet,
which SPEC-018 Revision 4 (section 6.3) records.

## 1. Overview & Scope

This specification applies only under the Fallout 2 profile (`game =
"fallout2"`). Under `fallout1` nothing here applies, and SPEC-005 to SPEC-014
are unchanged word for word. `[Must-match]`

It is written as a set of **deltas** against the Fallout 1 specifications,
section by section. "As SPEC-014 §3.x" means that the Fallout 1 rule applies
unchanged under the Fallout 2 profile, with its art, message files and data
read from the Fallout 2 databases (SPEC-018 §1). Where Fallout 2 differs, this
document gives the replacement rule.

It covers:

- the interface: the main menu, the interface bar and indicator boxes, the
  action menu, inventory, loot and barter, the dialogue control panel, the
  character screen, the Pip-Boy, options and preferences, save and load, and
  the world map and town map screens (section 3.2 to 3.13);
- party control: membership, the per-member control record, dispositions and
  the two control panels, the combat-AI deltas against SPEC-009, party-member
  level-ups, and how all this maps onto the engine's enhanced companion
  mechanism (sections 3.14 to 3.18);
- the car: acquisition, where it is, fuel, travel speed, the trunk and
  refueling (section 3.19);
- game-flow deltas: startup, new game, death, endgame, credits and the movie
  table (sections 3.2, 3.4, 3.20, 3.21).

Out of scope, and owned elsewhere:

| Subject | Owner |
| --- | --- |
| Mounting, archives | SPEC-018 §1, SPEC-015 |
| Maps and prototypes | SPEC-016 |
| Host words `0x8156`-`0x8183`, and the `metarule` and `metarule3` rule numbers this spec relies on | SPEC-017 |
| Save layout, including where the state of section 2.4 is stored | SPEC-018 |
| Data tables and their grammar: party table, world-map tiles and terrain, city table, map table, quest, holodisk, karma and reputation tables, endgame slides, new-game data | SPEC-018 |
| World-map random-encounter selection, its frequency tables, encounter population spawning, and dropping to terrain | SPEC-018 §5.6-§5.7 |

**Compatibility tags.** Every rule carries one tag.

- `[Must-match]`: documented Fallout 2 behavior that a player can observe, and
  that the engine must reproduce as written.
- `[May-improve]`: a reference defect that the engine replaces with the
  stated safe rule, as in SPEC-014.
- `[Safe rule]`: how the engine handles invalid, missing or out-of-range
  input. It refuses or clamps; it never reads out of bounds.
- `[Team A default]`: a detail that public documentation of Fallout 2 does
  not fix, chosen by Team A so that the engine is deterministic. The
  implementer must build it exactly as written, so that tests are stable. It
  is **not** claimed to match the original program. Every such default is
  listed in section 5.

**Conventions.** As SPEC-014 §1 (coordinates, fonts 100-104, color keys,
sounds, frames, the red button). Color keys are resolved through the Fallout 2
`color.pal`, so the bracketed palette indexes of SPEC-014 do not apply here.
Fallout 2 art is named, and resolved through `art/intrface/intrface.lst` by
name (section 2.2), except where SPEC-018's data gives an `intrface.lst` row
index directly (world-map tiles, town pictures and labels, karma-variable and
endgame art).

**Terms.** A **city** here is what SPEC-017 and SPEC-018 call an **area**:
one `[Area NN]` record of SPEC-018's `city.txt` (SPEC-018 §4.3). The city id
is the area number. A city's display name is its `area_name`.

### Revision history

- **R0 (2026-10-25).** First version, by Team A.
- **R1 (2026-10-25).** Reconciliation with SPEC-017 and SPEC-018 (decisions
  D-27 to D-31): no campaign-global writes, no carry amount, gated Play,
  SPEC-018's data and save fields referenced by section, and the terrain
  schedule and entrance placement corrected to SPEC-018's rules.
- **Revision 3 (2026-10-26).** Premade portraits: their retail size
  (section 2.2) and their place in the monitor window (section 3.3, D-32).
  Drafted by the coordinator; re-measured and approved by Team A.

## 2. Data, Constants and State

### 2.1 Numeric Conventions

All quantities are signed 32-bit integers, and divisions truncate toward zero,
as SPEC-014 §2.1. Percentages of a maximum are `trunc(max × p / 100)`.
`[Must-match]`

### 2.2 Interface Art Resolution

Fallout 2's `intrface.lst` has a different order from Fallout 1's, so this
spec names art by file name only. `[Team A default]`

1. An art name is looked up case-insensitively among the rows of
   `art/intrface/intrface.lst`. Its zero-based row is the list index used to
   build the art identifier (SPEC-001, SPEC-007 §2.1).
2. Where a row below lists several candidate names, the first one present in
   the list is used.
3. When no candidate is present, or the FRM fails to load, the engine uses
   the stated **stand-in**, and logs one diagnostic per art name per session.
   It never fails to open a screen because an art file is missing. `[Safe rule]`
4. The default stand-ins are: for a button, a 15-by-16 rectangle filled with
   key 18979, with a one-pixel key 14723 border, or the red button when that
   loads; for a background, a rectangle of the stated size filled with key 0;
   for an icon, the engine-drawn icon of the enhanced layer (roadmap part 2)
   when one exists, otherwise a blank. `[Safe rule]`

The Fallout 2 art the engine uses, beyond the names it shares with SPEC-014:

| Role | Candidate names | Stand-in | Section |
| --- | --- | --- | --- |
| Interface bar | `iface.frm` | Background 640 by 100 | 3.5 |
| Push icon pair | `pushn.frm` / `pushh.frm` | Enhanced push icon | 3.6 |
| Loot Take All button | `invmaup.frm` / `invmadn.frm`, then `takeallu.frm` / `takealld.frm` | The enhanced Take All button | 3.7 |
| Party dialogue panel | `di_talkp.frm` | `di_talk.frm` | 3.8 |
| Combat control panel | `control.frm` | Background 640 by 190 | 3.15 |
| Custom settings panel | `custom.frm` | Background 300 by 190 | 3.15 |
| World-map tiles | The row that the tile's `art_idx` gives (SPEC-018 §4.4.2) | Background 350 by 300, key 0 | 3.12 |
| Day and night dial | `wmdial.frm` | None drawn | 3.12 |
| Town-list scroll arrows | `wmlstup.frm` / `wmlstdn.frm`, then `uparwoff.frm` / `dnarwoff.frm` | Red button | 3.12 |
| Town pictures and town labels | The rows that the city's `townmap_art_idx` and `townmap_label_art_idx` give (SPEC-018 §4.3) | Picture: key 0; label: engine text (section 3.12.3) | 3.12, 3.13 |

These names are Team A's best knowledge and must be checked against a local
corpus by the asset audit (section 5, D-2). `[Team A default]`

**Retail audit (Revision 2).** The retail `intrface.lst` lists all 12 roles,
so every role resolves with no stand-in. The names and first-frame sizes in
use:

| Role | Name used | Size | Frames |
| --- | --- | --- | --- |
| Interface bar | `iface.frm` | 640 by 99, so h = 99 (section 3.5) | 1 |
| Push icon pair | `pushn.frm` / `pushh.frm` | 40 by 40 | 1 |
| Loot Take All button | `invmaup.frm` / `invmadn.frm` (`takeallu.frm` / `takealld.frm` are absent) | 39 by 41 | 1 |
| Party dialogue panel | `di_talkp.frm` | 640 by 190 | 1 |
| Combat control panel | `control.frm` | 640 by 190 | 1 |
| Custom settings panel | `custom.frm` | **640 by 190** | 1 |
| Day and night dial | `wmdial.frm` | 58 by 29 | 24 |
| Town-list scroll arrows | `uparwoff.frm` / `dnarwoff.frm` (`wmlstup.frm` / `wmlstdn.frm` are absent) | 11 by 14 | 1 |
| Main menu | `mainmenu.frm` | 640 by 480 | 1 |

The candidate order of the table above is kept, so an install that has the
first candidate still uses it. The custom panel is the one size that differs
from this spec's stand-in. Section 3.15.3 follows the art (Revision 2).

**Premade selection art (Revision 3).** The names are SPEC-014 §3.7's, read
by file name under `art/intrface/`:

| Role | Name used | Size | Frames | Notes |
| --- | --- | --- | --- | --- |
| Selection background | `pickchar.frm` | 640 by 480 | 1 | As Fallout 1 |
| Premade portraits | `combat.frm`, `stealth.frm`, `diplomat.frm` | **592 by 260** each | 1 | No index-0 pixel. Each is the whole monitor picture, the edge of its bezel included. Its border best fits `pickchar.frm`'s bezel with its top-left at screen (27, 23) (see below) |

All four files come from `master.dat`; `patch000.dat` overrides none of them.
No other `art/intrface/` frame in `master.dat` is 592 by 260. Fallout 1's
portraits of the same names are 212 by 187.

**How the fit was measured.** Each portrait was laid over `pickchar.frm` at
every top-left within 40 pixels of (27, 23), 81 by 81 offsets in all. At each
offset, the portrait's border ring was compared with the background beneath
it: by identical palette indices, and by mean RGB distance in `color.pal`'s
6-bit units. Ring widths of 1, 2, 4 and 8 pixels were tried.

- **The result.** For all three portraits and every ring width, the best
  offset by both measures is (27, 23), and the runner-up is always one
  pixel away.
- **The 2-pixel ring.**
  - At (27, 23), 67.1 to 68.1 per cent of the 3392 ring pixels are
    identical, with a mean distance of 2.80 to 2.86.
  - At the best one-pixel neighbours, the figures are 40.5 to 41.4 per
    cent and 5.99 to 6.05.
  - At (27, 24) and (27, 22), they are 38 to 40 per cent and 6.2 to 7.0.
- **The 1-pixel edges at (27, 23).** The left and right edges are about 75
  per cent identical, the bottom edge 67 to 71 per cent, and the top edge
  55 per cent.
- **How decisive.** The fit is decisive about the offset: it beats the
  runner-up by 23 to 29 percentage points, for every portrait and every
  ring width. It is not a pixel-for-pixel identity. About a third of the
  border pixels differ slightly in color, which suggests the portraits'
  bezel was drawn separately from the background's, not cut from it.

### 2.3 Messages and Engine Strings

Message files are under `text/<language>/game/` as SPEC-014 §2.4. This spec
uses the following, with Team A default IDs:

| File | IDs | Use |
| --- | --- | --- |
| `misc.msg` | 9-14 | Main-menu labels INTRO, NEW GAME, LOAD GAME, OPTIONS, CREDITS, EXIT (section 3.2) |
| `misc.msg` | 20 | Main-menu copyright line |
| `intrface.msg` | 100-104 | Indicator labels SNEAK, LEVEL, ADDICT, POISONED, RADIATED (section 3.5) |
| `pipboy.msg` | 500 + movie index | Video Archives titles (section 3.10) |

A message that is missing is replaced by the matching **engine string**
below, in English, and one diagnostic is logged. An engine string is also used
wherever this spec names one directly. `[Team A default]`

| Key | Text |
| --- | --- |
| E-menu-0 to E-menu-5 | `INTRO`, `NEW GAME`, `LOAD GAME`, `OPTIONS`, `CREDITS`, `EXIT` |
| E-ind-0 to E-ind-4 | `SNEAK`, `LEVEL`, `ADDICT`, `POISONED`, `RADIATED` |
| E-push-blocked | `There is no room to push %s.` (`%s` is the critter's name) |
| E-level | `%s has gained a level.` |
| E-fuel-out | `Your car is out of fuel.` |
| E-ctl-title | `COMBAT CONTROL` |
| E-ctl-weapon, E-ctl-armor | `Weapon`, `Armor` |
| E-ctl-best | `USE BEST WEAPON` |
| E-ctl-custom | `CUSTOM` |
| E-ctl-talk | `TALK` |
| E-ctl-done | `DONE` |
| E-disp-* | `Custom`, `Coward`, `Defensive`, `Aggressive`, `Berserk` |
| E-row-* | `Area Attacks`, `Run Away`, `Weapon Preference`, `Distance`, `Attack Who`, `Chem Use` |
| E-val-* | One per value token of section 2.5, written as the token with `_` replaced by a space and the first letter capitalized, for example `be_absolutely_sure` → `Be absolutely sure` |
| E-hp, E-ac, E-lvl | `HP %d/%d`, `AC %d`, `Level %d` |

### 2.4 State Introduced by This Specification

The engine keeps the following game state. It is part of a saved game, and
SPEC-018 stores it: the party list, control records, level stages and
retained records in SPEC-018 §7.5; the four car values in SPEC-018 §7.4; the
played-movie record as SPEC-018 §7.3 order 22. `[Must-match]`

| State | Type and range | Initial value at a new game |
| --- | --- | --- |
| Party list | Ordered object list, the player first, at most 32 entries (section 3.14) | The player only |
| Control record per party member | Section 2.5 | Created on joining |
| Retained control records | Records kept for objects that left with `party_remove` (section 3.14) | None |
| Level stage per party member | 0 to the member's level-PID count | 0 on joining, unless already stored |
| `has_car` | 0 or 1 | 0 |
| `car_area` | City id, or -1 | -1 |
| `car_map` | Map number, or -1 | -1 |
| `car_fuel` | 0 to 80000 | 0 |
| Played-movie record | 17 bytes, one per movie index (section 3.21) | All 0 |
| World-map town-list scroll offset | 0 or more; **not saved**, 0 after every load | 0 |

The world-map known, visited and entrance states, the city positions, the
world position and the discovery grid are SPEC-018's (SPEC-018 §5.8, saved
in §7.4).

### 2.5 The Control Record

Each party member other than the player that has a record in SPEC-018 §4.5's party
table has one control record. Its fields and their value tokens are those of
Fallout 2's public `party.txt` and `ai.txt` keys. `[Must-match]`

| Field | Value tokens, in panel order |
| --- | --- |
| `disposition` | `custom`, `coward`, `defensive`, `aggressive`, `berserk` |
| `area_attack_mode` | `always`, `sometimes`, `be_sure`, `be_careful`, `be_absolutely_sure` |
| `run_away_mode` | `none`, `coward`, `finger_hurts`, `bleeding`, `not_feeling_good`, `tourniquet`, `never` |
| `best_weapon` | `no_pref`, `melee`, `melee_over_ranged`, `ranged_over_melee`, `ranged`, `unarmed`, `unarmed_over_thrown`, `random` |
| `distance` | `stay_close`, `charge`, `snipe`, `on_your_own`, `stay` |
| `attack_who` | `whomever_attacking_me`, `strongest`, `weakest`, `whomever`, `closest` |
| `chem_use` | `clean`, `stims_when_hurt_little`, `stims_when_hurt_lots`, `sometimes`, `anytime`, `always` |

Tokens are compared case-insensitively. The party table lists, per member,
which tokens of each field the player may choose (its **allowed set**). An
unknown token in the data is dropped with a diagnostic. `[Safe rule]`

**Neutral record.** `custom`, `always`, `none`, `no_pref`, `on_your_own`,
`whomever`, `anytime`. With these values every rule of section 3.16 reduces
exactly to SPEC-009. `[Must-match]`

**Initial values on joining** (`party_add`, section 3.14), in this order of
preference, field by field: `[Team A default]`

1. the value stored for this member in a loaded save, when the member rejoins
   within the same game (the record survives `party_remove`, section 3.14);
2. the value of the same-named key in the member's resolved AI packet. This
   source never applies in this revision, because SPEC-018 §4.7 leaves the
   packets' Fallout 2 keys unread;
3. the neutral record's value.

A member without a party-table record has no control record and uses SPEC-009
unchanged. `[Must-match]`

### 2.6 Constants

`[Team A default]` unless tagged.

| Constant | Value |
| --- | --- |
| Party list capacity, player included | 32 |
| Car fuel maximum | 80000 `[Must-match]` |
| Fuel used per car travel iteration | 100 |
| Car pixel steps per iteration, terrain difficulty d | `max(1, trunc(4 / d))` |
| Town-list visible rows | 7 |
| City circle radius, by city size | Small 7, Medium 13, Large 20 pixels |
| Run-away fractions f (percent of maximum HP) | `coward` 100, `finger_hurts` 90, `bleeding` 75, `not_feeling_good` 50, `tourniquet` 25 |
| Combat follow threshold | `stay_close` 3; `on_your_own`, `charge`, `snipe` 5; `stay` none |
| Stay-close leash | 5 hexes from the player |
| Snipe minimum range | 4 hexes |

### 2.7 Fallout 1 Campaign Hooks Disabled Under Fallout 2

Several Fallout 1 rules read or write fixed campaign-global indexes. Those
indexes mean something else in Fallout 2's `vault13.gam`, so none of these
rules runs under the Fallout 2 profile. `[Must-match]`

| Fallout 1 rule | Under Fallout 2 |
| --- | --- |
| SPEC-014 §3.39 "Campaign checks on every pass" (water, vats, the Master's lair: globals 10, 17, 18, 55, 101, 147, 155, 308, 309) | Not run |
| SPEC-012 §3.12 water countdown at midnight (globals 10, 101, the water-movie byte) | Not run; the other midnight work (unjamming locks) still runs |
| SPEC-014 §3.41.1 vault-water note (`pip2.frm`, globals 10, 101) | Never drawn |
| SPEC-014 §3.18 preferences cover gated by global 603 | Never drawn; the fifth knob row is always shown and clickable |
| SPEC-014 §3.30 fixed slide table, and its global 299 reset | Replaced by section 3.20 |
| SPEC-014 §3.42 Karma folder rows tied to globals 155-158 and 189-194 | Replaced by section 3.9 |
| SPEC-014 §3.39 and SPEC-008 §3.5 writes of campaign indexes 32 and 65 on map entry, and `load_map`'s write of its entrance parameter (SPEC-005) | Not written (section 3.12.6, SPEC-017 §3.1, SPEC-018 §5.1) |
| SPEC-008 §3.7 fixed town table, hotspot catalog, destroyed-area substitutions; SPEC-008 §3.6 fixed grids; SPEC-014 §3.14 66-entry location table; SPEC-014 §3.41.4 related-map table | Replaced by SPEC-018's city, world-map and map tables |
| SPEC-014 §3.40 Vault 13 tick-864000 entrance rule | Not run |

## 3. Behavioral Rules & Edge Cases

### 3.1 Section Map

Every SPEC-014 section, and what applies under Fallout 2:

| SPEC-014 section | Under Fallout 2 |
| --- | --- |
| 3.1 Startup | As SPEC-014, with the Fallout 2 movie table (section 3.21) |
| 3.2 Palette fades | As SPEC-014 |
| 3.3-3.6 Main menu, loop, choices, attract mode | Section 3.2 |
| 3.7 Premade selection | Section 3.3 |
| 3.8-3.11 Editor in creation mode | As SPEC-014; content per SPEC-018 (section 3.3) |
| 3.12 New-game hand-off | Section 3.4 |
| 3.13 Dialog box | As SPEC-014 |
| 3.14-3.16 Save and load | As SPEC-014, except the location names (section 3.11) |
| 3.17 Options, pause, quit | As SPEC-014 |
| 3.18 Preferences | As SPEC-014, except the cover (section 2.7) |
| 3.19-3.25 Interface bar, monitor, counters, pips, item slot, indicators, combat panel | Section 3.5 |
| 3.26-3.27 Cursors and in-world cursor | As SPEC-014, with the bar-height rule of section 3.5.1 |
| 3.28 Action menu | Section 3.6 |
| 3.29 Edge scrolling | As SPEC-014 |
| 3.30-3.32 Endgame, ending movie, credits | Section 3.20 |
| 3.33 Automap window | As SPEC-014 |
| 3.34 Inventory and use-on windows | Section 3.7 |
| 3.35-3.38 Working map set, roofs, cutout, called shot | As SPEC-014 |
| 3.39 World map | Section 3.12 |
| 3.40 Town map | Section 3.13 |
| 3.41 Pip-Boy | Section 3.10 |
| 3.42-3.43 Character screen and sheet | Section 3.9 |
| 3.44 Draw order | As SPEC-014 |
| 3.45 Dialogue review | Section 3.8 |
| 3.46 Floating text | As SPEC-014 |

### 3.2 Startup and Main Menu

**Startup.** As SPEC-014 §3.1, except step 9: the movies are index 0
(`iplogo.mve`, flag `0x01`) and then index 1 (`intro.mve`, flags 0) of
section 3.21. `[Must-match]`

**Main-menu window.** As SPEC-014 §3.3, with these differences. `[Team A
default]` for the geometry; `[Must-match]` for the six buttons.

| Element | Value |
| --- | --- |
| Background | `mainmenu.frm`, 640 by 480, opaque |
| Buttons i = 0-5 | Six buttons, INTRO, NEW GAME, LOAD GAME, OPTIONS, CREDITS, EXIT. Rectangle (30, 19 + 41i) 26 by 26; `menuup.frm` / `menudown.frm`; only nonzero pixels of the up image are clickable |
| Label i | `misc.msg` 9 + i, font 104, key 21091, top-left (126 - trunc(w / 2), 20 + 41i) |
| Copyright | `misc.msg` 20, font 100, key 21204, at (15, 460); not drawn when the message is missing |
| Version | Text `FALLOUT II 1.02d`, font 100, key 21204, right edge at x = 615, y = 460 |
| Hotkeys | `i`, `n`, `l`, `o`, `c`, `e`, either case, for buttons 0-5 |
| Music | `sound/music/07desert.acm`, as SPEC-014 §3.4; a missing file plays nothing (SPEC-010) |

**Loop and choices.** As SPEC-014 §3.4 and §3.5, with one added choice.
`[Must-match]`

| Choice | Behavior | Then |
| --- | --- | --- |
| Options | Fade out and hide the menu. Open the preferences screen of SPEC-014 §3.18 directly, without the options menu of §3.17. DONE and CANCEL behave as there. A confirmed quit from inside it returns here | Loop top |

Shift+Credits (Quotes), Esc (Exit), the 120-second timeout, `d`, brightness
keys and Ctrl+R are as SPEC-014 §3.4-§3.6. `[Must-match]`

### 3.3 Premade Selection and Character Creation

**Premade selection.** As SPEC-014 §3.7: the same art names, the same three
entries in the same order (`premade/combat.gcd`, `stealth.gcd`,
`diplomat.gcd`, with their `.bio` files and portraits `combat.frm`,
`stealth.frm`, `diplomat.frm`), and the same keys. The one difference: **no ID
text** is drawn (the SPEC-014 "ID text" column and its y = 252 line).
`[Team A default]`

**Portrait (Revision 3).** The portrait is not placed by SPEC-014's
`(150 - trunc(w / 2), 240 - h)`, which suits only Fallout 1's 212-by-187 art.

- **Placement.** The **monitor window** is the screen rectangle 592 by 260
  whose top-left is (27, 23): x 27 to 618 and y 23 to 282, inclusive. The
  portrait's top-left is drawn at the window's top-left, (27, 23).
  `[Team A default]` (D-32, measured)
- **Clipping.** The portrait is clipped to the monitor window: no pixel is
  drawn outside it, whatever the art's size. `[Safe rule]` (D-32)
- **Drawing.** The scan-line effect and index-0 transparency are SPEC-014's,
  unchanged: odd rows of the art (rows 1, 3, 5 and so on, counted from the
  art's own top) are not drawn and show the background. The retail
  portraits have no index-0 pixel. Nothing measured shows whether Fallout 2
  keeps the scan lines, so they are kept from SPEC-014. `[Team A default]`
  (D-32)
- **Text.** The name, SPECIAL, derived-stat, tag, trait and bio lines are
  drawn at SPEC-014's positions, over the portrait. `[Team A default]`

Under Fallout 1 nothing changes. SPEC-014 §3.7's formula places the 212 by
187 portraits at (44, 53), and they end at (255, 239), inside SPEC-014's
monitor area. An engine may clip them to that area, which has no visible
effect.

The sheet is loaded with SPEC-018 §6.3's Fallout 2 layout (Revision 4), so
the three tag lines show the sheet's first three tags.

**Editor.** As SPEC-014 §3.8-§3.11. The trait, skill, perk and stat names
come from the Fallout 2 message files. Their rules are SPEC-006's and
SPEC-011's, provisionally (SPEC-018 §4.7, §6.3). The layout, counters,
validation and dialogs are unchanged. `[Must-match]`

### 3.4 New-Game Hand-Off

When selection returns "start", SPEC-014 §3.12 runs with these replacements,
which are SPEC-018 §6.6's. `[Must-match]`

| SPEC-014 §3.12 step | Under Fallout 2 |
| --- | --- |
| Before selection | SPEC-018 §6.6 step 1: the tables and the campaign initializer are parsed; a refusal returns to the menu with a message naming the file and line |
| 2 | Play movie index 2, `elder.mve` (SPEC-018 §6.4), with flag `0x04` |
| 7 | Load `maps/artemple.map` by name. The player is placed at the map header's default entrance (SPEC-003, SPEC-016; SPEC-018 §6.5) |
| 8 | Start that map's city music from SPEC-018 §5.2 |
| Initial state | SPEC-018 §6.1's clock (tick 264600) and calendar (tick 0 is 25 July 2241); campaign globals from `data/vault13.gam`; SPEC-018 §6.5's world state; party list the player only; no car (section 2.4); played-movie record all 0. Everything else as SPEC-014 §3.12 |

**Revision 2:** SPEC-017 Revision 3 opens the script gate. New Game and Load
Game run scripts, and the paragraph below no longer applies. A gated save
is refused (SPEC-018 §7.2). The main menu, premade selection and editor are
unchanged.

**While Fallout 2 scripts were gated** (SPEC-017 §8.5), New Game and Load
Game worked as above. The session follows SPEC-017 §8.7: no script runs, and
after the hand-off completes, and after each successful load, the message
monitor prints SPEC-017 §8.7's one-line notice. The main menu, premade
selection and editor (sections 3.2 and 3.3) need no scripts and are
unchanged. `[Safe rule]`

### 3.5 Interface Bar and Indicator Boxes

#### 3.5.1 Bar Geometry

Let h be the height of the loaded `iface.frm` (from its FRM header). When h is
below 60 or above 100, h is taken as 100 and a diagnostic is logged.
`[Safe rule]`

- The bar window is at screen (0, 479 - h), 640 by h. With h = 100 that is
  SPEC-014's (0, 379). `[Team A default]`
- Every element of SPEC-014 §3.19-§3.25 keeps its **screen** position: its
  window-relative x is unchanged and its window-relative y is the SPEC-014
  value minus (100 - h). For h = 99 every y drops by 1; for example the
  Inventory button is at window (211, 40), screen (211, 420). `[Team A
  default]`
- The map window is 640 by (481 - h). Every SPEC-014 test "y at most 379" for
  the map area becomes "y at most 479 - h". `[Team A default]`
- The indicator-box window is at screen (0, 479 - h - 21). `[Team A default]`
- Buttons, art names, sounds, hotkeys, the message monitor, the HP and AC
  counters, the AP pips, the active item slot, the combat panel and the
  screenshot key are otherwise as SPEC-014 §3.19-§3.25. `[Must-match]`

#### 3.5.2 Indicator Boxes

As SPEC-014 §3.24, with five possible boxes instead of three. `[Team A
default]`

| Order | Label | Color | Lit when |
| ---: | --- | --- | --- |
| 1 | ADDICT (`intrface.msg` 102) | Key 31744 | The player has at least one active addiction (SPEC-011) |
| 2 | SNEAK (100) | Key 992 | As SPEC-014 |
| 3 | LEVEL (101) | Key 992 | As SPEC-014 |
| 4 | POISONED (103) | Key 31744 | The player's poison level is above 0 |
| 5 | RADIATED (104) | Key 31744 | The player's radiation level is above 0 |

Lit boxes are drawn in that order; at most five slots exist. Box k (from 0) is
at x = `130k - 3(k + 1)`, the first cut to 127 pixels at x = 0, and the window
width is 127 times the number of lit boxes, as SPEC-014 §3.24.

### 3.6 Action Menu

As SPEC-014 §3.28, with one entry added. `[Team A default]`

- **Push.** For a critter that is in the party list, is not the player, and is
  neither dead nor knocked out, out of combat only, a **Push** entry is
  inserted directly after the critter's first entry (Talk, or Use). Its icons
  are the push pair of section 2.2.
- Choosing Push runs the talk approach of SPEC-014 §3.27 (distance below 9 and
  an unblocked line, approached first when either test fails, with
  `misc.msg` 2000 on failure). Then:
  1. Request the target's `push_p_proc` with the player as source. An
     override stops here.
  2. Otherwise let D be the hex direction from the player's hex to the
     target's hex (SPEC-003 §3.5). Try the target's neighbor in direction D,
     then `(D + 1) mod 6`, then `(D + 5) mod 6`. The first neighbor that is on
     the grid and not blocked (SPEC-007 §3.1 blocker test) is the destination.
  3. With a destination, the target walks there through one ordinary walk
     sequence (SPEC-007), with no AP and no sound of its own.
  4. With none, the monitor prints E-push-blocked and nothing moves.
- No random draw is made. `[Must-match]`

### 3.7 Inventory, Loot and Barter

**Inventory and use-on windows.** As SPEC-014 §3.34, with one change: the
total-weight line reads `inventry.msg` 20, a space, the total carried weight,
`/`, and the carry limit (SPEC-011 §3.14). The line is drawn in key 31744
instead of key 992 when the total exceeds the limit. `[Team A default]`

**Loot window.** As SPEC-013 §3.16, with a **Take All** button. `[Must-match]`
that it exists; `[Team A default]` for its art and place.

- The button uses the Take All pair of section 2.2. It sits at the same
  window rectangle as the engine's enhanced Take All button (roadmap part 2
  b), with the art's own size.
- It does exactly what Shift+A does (SPEC-013 §3.16, "Take All"), including the
  weight test and the silent refusal. It is absent in steal sessions.

**Barter with a party member ("trade").** `[Must-match]` unless tagged.

- A barter request in dialogue (the `b` key, the Barter button, or
  `gdialog_barter`) whose target is a party member with a control record opens
  the barter window **without** the SPEC-013 §3.18 test of the prototype's
  barter flag.
- In such a session, `m` (Offer) always succeeds when either table is nonempty:
  both tables are exchanged at once, without the price comparison of SPEC-011
  §3.19, and `inventry.msg` 27 is shown. The totals are still drawn. `[Team A
  default]` for the message.
- The member's equipped weapon and armor are hidden and restored as a
  merchant's are (SPEC-011 §3.17).
- Everything else is as SPEC-013 §3.18.

### 3.8 Dialogue Control Panel

As SPEC-014 §3.45, with these differences.

- There is **no Tell Me About** button, and the `a` key does nothing in the
  reply and choice loop. SPEC-006 §3.9's Ask About lookup is never reached
  under Fallout 2. `[Must-match]`
- Barter and Review are as SPEC-014 §3.45. `[Must-match]`
- When the dialogue target is a party member with a control record, the
  panel art is `di_talkp.frm` (section 2.2) instead of `di_talk.frm`, and a
  **Combat Control** button is added at the old Tell Me About place, (593, 116)
  14 by 14, with the Barter button's art and sounds. It acts on release; the
  key `c` does the same in the reply and choice loop. It opens the control
  panel of section 3.15. `[Team A default]` for the art name, place and key.

### 3.9 Character Screen

As SPEC-014 §3.42 and §3.43, except the Karma and Kills folders. `[Must-match]`
for the content; `[Team A default]` for the order.

- **Karma folder.**
  - Row 0: `editor.msg` 1000 followed by the player's karma, as SPEC-014 §3.42
    row 0. Karma is the player's progression karma value (SPEC-008 §2.5 order
    13; SPEC-005 player-stat selector 4), not a campaign global.
  - Then one row: the general reputation title from SPEC-018 §4.6's
    `genrep.txt`, selected by the karma value. No title, no row.
  - There are no town-reputation rows in this revision: SPEC-018 §4.7 knows
    no data for them.
  - Then one row per record of SPEC-018 §4.6's `karmavar.txt` whose global is
    nonzero, in table order, with that record's name, picture and description
    for the info card.
  - Rows start at y = 373 after row 0 and step 11, as SPEC-014. Rows beyond
    the folder area are not drawn.
- **Kills folder.** As SPEC-014 §3.42, over SPEC-018 §7.3's kill types 0-18.
  Type t's name is `proto.msg` 1450 + t and its description `proto.msg` 1469
  + t, instead of 450-464 and 465-479. A missing name falls back to an
  English engine string: SPEC-003 §2.4's names for 0-14, then `Gecko`,
  `Alien`, `Giant Ant` and `Big Bad Boss` for 15-18. Up to 10 types fill the
  left column and the rest the right column. `[Team A default]`

### 3.10 Pip-Boy

As SPEC-014 §3.41, with these differences. `[Must-match]` unless tagged.

- The vault-water note is never drawn (section 2.7).
- **Availability.** On a map whose `maps.txt` record has `pipboy_active=No`
  (SPEC-018 §4.2), the Pip-Boy button and its key do nothing. `[Team A
  default]`
- **Status page.** The quest groups, quests and holodisks come from SPEC-018
  §4.6's `quests.txt` and `holodisk.txt`, with SPEC-018 §5.9's rules: a quest
  is hidden, active or completed by its thresholds, and a holodisk is held
  when its global is **positive**. They are placed as SPEC-014 §3.41.3 places
  them. An active quest is drawn as an ordinary one; a completed quest is
  drawn in key 8804, struck through. `[Team A default]`
- **Automaps page.** Maps are grouped by their city (SPEC-018 §5.2's area of
  a map) instead of SPEC-014's related-map table, and each group lists the
  captured elevations of every map of that city, in ascending map number.
  `[Team A default]`
- **Video Archives page.** One entry per movie index 2 through 15 whose played
  byte is set (section 3.21), in index order, with text `pipboy.msg` 500 +
  index. The SPEC-012 §3.12 Pip-Boy selection scans indexes 2 to 15 instead of
  3 to 13; when no movie qualifies, nothing plays. `[Team A default]`
- Alarm Clock, start screen, holidays, screen saver and keys are as SPEC-014.

### 3.11 Options, Preferences, Save and Load

- Options menu, pause window and quit confirmation: as SPEC-014 §3.17.
  `[Must-match]`
- Preferences: as SPEC-014 §3.18, without the cover (section 2.7).
  `[Must-match]`
- Save and load screens: as SPEC-014 §3.14-§3.16. The info box's location line
  is the saved map's location name (SPEC-018 §5.2); there are no elevation
  names, and an unknown map shows an empty location. The slot files and their
  layout are SPEC-018 §7's.
- A slot that fails SPEC-018 §7.2's header checks (a Fallout 1 slot, another
  layout tag, another G, or a gated save once the script gate is open) is
  shown in SPEC-014 §3.14's **old version** state: `lsgame.msg` 113 in the
  list and 116 in the info box. Loading it plays `iisxxxx1` and shows title
  134 with body 136. A slot whose header is short or whose signature fails is
  shown as corrupt, as SPEC-014. `[Must-match]`

### 3.12 World Map Screen

The rules of SPEC-014 §3.39 apply, with SPEC-018's world-map and city tables
(SPEC-018 §4.3, §4.4, §5)
in place of SPEC-008 §3.6 and §3.7, and with the replacements below.

#### 3.12.1 World and Tiles

- The world is the grid of tiles that SPEC-018's world-map table defines
  (SPEC-018 §4.4.2, §5.3):
  `num_horizontal_tiles` tiles per row, each tile 350 by 300 pixels, made of
  7 by 6 subtiles of 50 by 50 pixels. With 4 tiles per row and 20 tiles the
  world is 1400 by 1500, as Fallout 1's. `[Must-match]`
- Each tile draws its own art (SPEC-018 §4.4.2's `art_idx`). The view at
  (22, 21), 450 by 442, shows world pixel (x, y) at `(x - vx + 22, y - vy + 21)`
  as SPEC-014. The view origin is clamped to 0 through world width - 450 and
  0 through world height - 442. `[Must-match]`
- A subtile is a discovery cell. Its three discovery values (0 black, 1 dim,
  2 clear) are drawn as SPEC-014 §3.39 "Drawing a pass" step 2. The reveal
  around the party is SPEC-008 §3.5's 3-by-3 (5-by-5 with Scout), and the
  per-subtile fill rules are SPEC-018 §5.5's. SPEC-008 §3.5's fixed west and
  ocean reveal does not apply. `[Must-match]`

#### 3.12.2 Cities

A city has a current world position (X, Y), a size (Small, Medium, Large), a
known state (0 unknown, 1 known, 2 visited), a lock state and up to 32
entrances (SPEC-018 §4.3, §5.8). All of it except the runtime state is
SPEC-018 data; the position starts as `world_pos` and scripts may move it
(SPEC-017 §4.5).

- **Containment.** The party is in city c when `(px - X)² + (py - Y)² ≤ r²`,
  with (px, py) the party's world position and r the size's radius
  (section 2.6). When several cities contain the party, the first in city-table
  order wins. This replaces SPEC-008's "town cell" everywhere. `[Team A
  default]`
- **Drawing.** After step 2 of SPEC-014's pass and before the party marker,
  each **known** city is drawn as a one-pixel circle outline of radius r in key
  992, centered on its window position, clipped to the view. Its name (the
  city's display name, its `area_name`, SPEC-018 §4.3) is drawn below it in font 101, key 992, with
  shadow, centered on X, top at Y + r + 2. Unknown cities draw nothing.
  Containment also decides SPEC-018 §5.6's encounter suppression, for known
  and unknown cities alike.
  `[Team A default]`
- **Hover label.** As SPEC-014's hover label, with the text: inside a city, its
  display name when known, otherwise `worldmap.msg` 1004 "Unknown"; elsewhere,
  the terrain name of the party's subtile (SPEC-018 §4.4.2).
- **Locked cities.** A city whose lock state is set cannot be entered: the
  enter click and its town-list button do nothing (their sounds still play).
  `[Team A default]`

#### 3.12.3 Side Panel

`[Team A default]` for the geometry.

| Element | Value |
| --- | --- |
| Date and time | As SPEC-014 §3.39: day (487, 12), month (513, 12), year (548, 12), time (593, 12) |
| Day and night dial | `wmdial.frm` at (532, 48), frame `trunc(hour × frames / 24)` for the clock's hour 0-23; not drawn when the art is missing |
| Town list | 7 rows. Row k (0-6) has a red button at (508, 138 + 27k), 15 by 15 (only the top 15 rows of the red button shown, as SPEC-014), and the city's label at (531, 138 + 27k) |
| List scroll | Up at (590, 112) and down at (590, 330), the arrow pair of section 2.2; red-button sounds |
| Fuel gauge | Section 3.19.4 |
| Town/World | (520, 439), 15 by 15, as SPEC-014 |

- The list holds every **known** city, in city-table order. Row k shows list
  entry `offset + k`. Up needs a positive offset; down needs `offset + 7` below
  the entry count. The offset is clamped into range whenever the list changes.
- The label is the city's label art (`townmap_label_art_idx`, SPEC-018
  §4.3) drawn transparently; without it, the city's display name in font 101,
  key 992.
- A row button behaves as SPEC-014's town button for that city: travel to the
  city's position when the party is not in it, enter it when it is.
  Destinations use SPEC-008 §3.7's jitter (four draws) around the city's world
  position. `[Must-match]`

#### 3.12.4 Entering a City

As SPEC-014 §3.39 "Entering a town or dropping to terrain", with these steps
for city c. `[Must-match]`

- **Not yet visited** (known state 1): load the map of c's lowest-numbered
  known entrance (SPEC-018 §5.8), place the player as section 3.13 says, and
  set c's known state to 2. With no known entrance, nothing loads and the
  screen stays open. `[Safe rule]`
- **Visited:** c becomes the current city, and the town map of c opens
  (section 3.13).
- **Dropping to terrain** (a click on the party outside every city): the
  map and entrance are SPEC-018 §5.7's "Dropping to terrain".

#### 3.12.5 Travel

SPEC-008 §3.4's line stepping, walking mask, W and Q, time charging and queue
interruption apply, with two changes. `[Team A default]` unless tagged.

- **Terrain schedule.** On foot, SPEC-018 §5.4's schedule: each subtile's
  terrain has a difficulty d from 1 to 10; a terrain counter starts at 0 for
  each trip; each iteration charges Q ticks and subtracts 1 from the counter,
  and when the counter is then 0 or less, one pixel step is made and the
  counter becomes d. So d = 2 steps on iterations 1, 3, 5 and so on. This
  replaces SPEC-008's fixed desert, mountain and city schedule.
  `[Must-match]`
- **In the car:** section 3.19.3.

Script-directed town travel (SPEC-008 §3.4) is replaced: after a transition
loads a map of city c, the world position becomes c's current world position,
with no travel simulation, no time charge and no jitter. A map with no city
leaves the position unchanged. This is SPEC-018 §5.4's rule too. `[Team A
default]`

#### 3.12.6 Leaving to a Map

As SPEC-014 §3.39 "Leaving to a map", except step 2: no campaign global is
written (section 2.7). The map loads by name, and its number comes from
SPEC-018 §5.1. City music comes from SPEC-018 §5.2. `[Must-match]`

#### 3.12.7 Encounters

The encounter check, selection, tables and population are SPEC-018
§5.6-§5.7's. This spec fixes only how a selected encounter is presented: the
flash of SPEC-014 §3.39 "Encounters", special for an entry marked `Special`
and ordinary otherwise, then the map load. There is no avoidance prompt.
`[Must-match]`

#### 3.12.8 Unchanged

Opening, follow mode, scrolling, keys, travel dots, the Town/World button,
arrival at rest, the at-rest and travelling markers and the pause window are
as SPEC-014 §3.39. No key closes the world map. `[Must-match]`

### 3.13 Town Map Screen

As SPEC-014 §3.40, with SPEC-018's city table (SPEC-018 §4.3) in place of SPEC-008's hotspot
catalog. `[Must-match]` unless tagged.

- The picture is the art of c's `townmap_art_idx` (SPEC-018 §4.3), copied opaque at
  (20, 20) and seen through the frame's hole. A picture of another size is
  clipped to the hole. A missing picture is a failure (SPEC-014 §3.40).
- Each entrance of c whose state is on gets a marker built as SPEC-014's, with
  its anchor at the entrance's (x, y): the 164-by-25 button at (x - 57, y - 6),
  the triangle at (x + 12, y + 6).
- The label is the destination map's `lookup_name` (SPEC-018 §4.2); when
  that is absent, no label. `[Team A default]`
- **Choosing an entrance** while the party is in c: c becomes the current
  city and the entrance its current entrance; the entrance's map loads by
  name; the player is placed by SPEC-018 §4.3's entrance items: tile -1 uses
  the map header's whole default entrance; otherwise the entrance's tile,
  with its elevation (or the header's for -1) and its orientation (or the
  header's for -1). No campaign global is written. Exit grids on the
  placement hex behave as SPEC-013 §3.22 says. `[Must-match]`
- The side panel is the world map's (section 3.12.3). A known city's button
  switches the picture to that city, as SPEC-014.
- There are no destroyed-area rows or substitutions; scripts change entrance
  states instead (SPEC-017). `[Must-match]`

### 3.14 Party Membership

`[Must-match]` unless tagged.

- **Joining.** `party_add` (SPEC-005, `0x8124`) appends any non-null object
  not already in the list. It switches a critter to team 0 (SPEC-009 §3.15).
  When the member has a record in SPEC-018 §4.5's party table, a control
  record is created or restored (section 2.5) and its level stage set
  (section 3.17).
  When the list already holds 32 entries, the call does nothing and logs a
  diagnostic. `[Safe rule]`
- **Leaving.** `party_remove` (`0x8125`) removes the object. Its control record
  and level stage are kept as a **retained record**, keyed by the object's
  identifier, so that a member who rejoins keeps its settings. Retained
  records are saved (SPEC-018 §7.5). `[Team A default]`
- **Who can join.** The engine refuses no object. The documented Fallout 2
  rule, at most `trunc(effective Charisma / 2)` followers plus one with the
  Magnetic Personality perk, is applied by the campaign scripts, which read
  the party count through `metarule` rule 16. The engine never enforces it.
- **Counting.** Scripts see one count: all entries, the player included,
  through `metarule` rule 16 (SPEC-017 §5.1). No script word returns the
  **visible** count, which leaves out entries whose object flag `0x01`
  (hidden) is set; this spec uses it only in its own rules (section 3.16
  rule 9's friendlies, and section 4.15). `[Team A default]`
- **Hidden and non-critter members.** A hidden entry, or an entry whose object
  is not a critter (the car trunk, section 3.19.5), never takes a combat turn,
  never follows, and is never counted as visible when hidden. It travels with
  the party on every map change and elevation change: it is moved to the
  player's hex and elevation without the placement rule, keeps its hidden flag,
  and does not block. A visible non-critter entry is placed by SPEC-013 §3.22's
  placement rule like a critter.
- **Dead members.** When the player leaves a map, every party critter that is
  dead is removed from the list first and stays on the map as a corpse. Its
  control record is discarded, not retained. `[Team A default]`
- **Following out of combat** is done by the members' scripts. The engine adds
  only SPEC-013 §3.22's placement on elevation and map changes. In combat,
  following is section 3.16's step 6.

### 3.15 Dispositions and the Control Panels

#### 3.15.1 Dispositions

Choosing a disposition other than `custom` sets the six other fields to its
preset. Choosing `custom` changes nothing else. Changing any of the six fields
on the custom panel sets the disposition to `custom`. `[Team A default]`

| Disposition | Area attacks | Run away | Weapon | Distance | Attack who | Chem use |
| --- | --- | --- | --- | --- | --- | --- |
| `coward` | `be_absolutely_sure` | `coward` | `ranged` | `snipe` | `whomever_attacking_me` | `stims_when_hurt_little` |
| `defensive` | `be_sure` | `bleeding` | `ranged_over_melee` | `stay_close` | `whomever_attacking_me` | `stims_when_hurt_little` |
| `aggressive` | `sometimes` | `not_feeling_good` | `no_pref` | `on_your_own` | `closest` | `stims_when_hurt_lots` |
| `berserk` | `always` | `never` | `melee_over_ranged` | `charge` | `strongest` | `clean` |

A preset's values apply even when a value is outside the member's allowed set
for that field; the allowed set only limits what the custom panel offers.
SPEC-018's party table gives no packet number per disposition, so a
disposition never changes the member's AI packet number. `[Team A default]`

#### 3.15.2 Combat Control Panel

Opened from the Combat Control button (section 3.8). It replaces the dialogue
panel at screen (0, 290), 640 by 190, like the barter window, and the head
stays visible above it. Coordinates are window-relative. `[Team A default]`

| Element | Position | Content |
| --- | --- | --- |
| Background | (0, 0) | `control.frm`, or its stand-in |
| Title | (20, 8) | E-ctl-title, font 103, key 18979 |
| Name | (20, 28) | The member's name, font 101, key 992, clip 120 |
| Stats | (20, 42), (20, 54), (20, 66) | E-hp with current and maximum HP, E-ac with effective AC, E-lvl with level stage + 1; font 101, key 992 |
| Weapon box | (150, 26) 90 by 61 | Caption E-ctl-weapon above at (150, 14); the right-hand item's inventory art scaled into the box as SPEC-014 §3.34 slots |
| Armor box | (250, 26) 90 by 61 | Caption E-ctl-armor at (250, 14); the worn armor's art |
| Use Best Weapon | Red button (150, 100), caption E-ctl-best at (170, 100), font 103 | See below |
| Disposition rows k = 0-4 | Red button (380, 20 + 22k), label (400, 20 + 22k), font 101 | Tokens in section 2.5 order; the current one in key 32747, others key 992; a token outside the allowed set in key 1313 with its button disabled |
| Custom | Red button (380, 134), caption E-ctl-custom at (400, 134), font 103 | Opens the custom panel; disabled when `custom` is not allowed |
| Talk | (584, 162), 14 by 14, `di_rdbt2.frm` / `di_rdbt1.frm`, `ib2p1xx1` / `ib2lu1x1` | Returns to dialogue |

- **Keys.** `t` and Esc return to dialogue. `1`-`5` choose disposition rows 1-5
  when allowed. `u` is Use Best Weapon. Ctrl+Q, Ctrl+X and F10 open the quit
  confirmation. F12 takes a screenshot.
- **Use Best Weapon.** Run SPEC-009 §3.9's inventory search **without** the AP
  check, with section 3.16's weapon ranking for the member's `best_weapon`.
  When it finds a weapon other than the right-hand item, wield it in the right
  hand with its animation and no AP cost, and redraw the weapon box. Otherwise
  play `iisxxxx1`.
- Every button plays the red-button sounds unless stated. A disposition change
  redraws the rows.

#### 3.15.3 Custom Panel

A modal window the size of its background. With the retail `custom.frm`
(640 by 190) it sits at screen (0, 290), in the control panel's place
(Revision 2). With the stand-in it is at screen (170, 145), 300 by 190. The
rows and Done keep the window-relative positions below in both cases.
`[Team A default]`

- Background `custom.frm`, or its stand-in with a one-pixel key 992 border.
- Six rows j = 0-5, in section 2.5 field order from `area_attack_mode`: label
  E-row-j at (16, 20 + 22j), font 101, key 992; a red button at (150, 20 + 22j);
  the current value's E-val text at (170, 20 + 22j), key 32747.
- A press of row j's button sets the field to the first token of the
  member's allowed set that comes after the current value in section 2.5
  order, wrapping to the start of that order. A current value outside the
  allowed set counts by its own position in that order. When the allowed set
  is empty nothing changes, and when the search wraps back to the current
  value nothing changes either. Any change sets the disposition to `custom`.
- Done: a red button at (110, 160) with caption E-ctl-done at (130, 160); Enter
  and Esc also close. The control panel is redrawn on return.

### 3.16 Party Combat AI Deltas (SPEC-009)

These rules apply to an NPC turn (SPEC-009 §3.6) only when the critter is a
party member other than the player and has a control record. Otherwise
SPEC-009 is unchanged. With the neutral record (section 2.5), every rule below
yields SPEC-009's result. None of them makes a random draw, so SPEC-009 §3.19's
draw order is unchanged. `[Must-match]` that they apply and draw nothing;
`[Team A default]` for each rule's details.

Let K be the resolved packet and R the control record.

1. **Forced flee (§3.6 step 1).** The HP test uses a threshold from
   `run_away_mode`: `none` uses `K.min_hp`; the five fractional modes use
   `trunc(max HP × f / 100)` with f from section 2.6; the test is HP strictly
   below the threshold. `never` disables both the HP test and the
   `hurt_too_much` test. The fleeing-bit test is unchanged.
2. **Healing (§3.7).** By `chem_use`:

   | `chem_use` | Healing threshold B | Successful uses per turn |
   | --- | --- | --- |
   | `clean` | No healing step at all | 0 |
   | `stims_when_hurt_little` | `trunc(max HP × 75 / 100)` | Unlimited |
   | `stims_when_hurt_lots` | `trunc(max HP / 2)` | Unlimited |
   | `sometimes` | `trunc(max HP / 2)` | 1 |
   | `anytime` | `trunc(max HP / 2)` (SPEC-009) | Unlimited |
   | `always` | `trunc(max HP × 75 / 100)` | Unlimited |

   Only the drugs of SPEC-009 §3.7 (PIDs 40 and 144) are used, under every
   mode.
3. **Target (§3.6 step 2).** With no requested target, T is chosen by
   `attack_who` instead of `danger(self)`. First sort the snapshot from self as
   SPEC-009 §3.2 does. The **hostile candidates** are the entries E, in that
   order, for which all hold: E is not self, not dead, not the player and not
   in the party list; self perceives E (SPEC-009 §3.3); and E's last attacker is
   the player or a party member, or the last attacker of the player or of some
   party member is E, or E's intent bit `0x01` is set and E's team differs from
   the player's.

   | `attack_who` | T |
   | --- | --- |
   | `whomever` | `danger(self)` (SPEC-009) |
   | `whomever_attacking_me` | The first candidate whose last attacker is the player; if none, `danger(self)` |
   | `closest` | The first candidate; if none, `danger(self)` |
   | `strongest` | The candidate with the highest retaliation rating (SPEC-009 §3.4), the first in order among equals; if none, `danger(self)` |
   | `weakest` | The candidate with the lowest current HP, the first in order among equals; if none, `danger(self)` |

   A requested target (§3.6 step 3) is used unchanged.
4. **Max-distance stop (§3.6 step 4).** Skipped under `charge`. Otherwise
   unchanged.
5. **Follow (§3.6 step 6).** The threshold is section 2.6's by `distance`:
   approach the player when `dist(self, player)` is above it. Under `stay`
   there is no follow.
6. **Approach (§3.12).** By `distance`:
   - `stay`: every approach fails at once, with no movement and no taunt; where
     §3.12 would then flee, the loop returns failure instead. Environment
     searches for weapons and ammunition (§3.9, §3.13) find nothing.
   - `stay_close`: when `dist(self, player)` is above the stay-close leash
     (5), the approach is replaced by an approach to the player, without the
     taunt, and the loop returns failure.
   - Other modes: unchanged.
7. **Snipe retreat.** Under `snipe`, after T is chosen and before the attack
   loop: when the right-hand weapon's primary subtype is ranged and
   `dist(self, T)` is at most the snipe minimum range (4), let A be current AP
   minus the non-aimed AP cost of hit mode 2 (SPEC-007 §3.4). When A is above
   0, run SPEC-009 §3.8 steps 2-4 with D the direction from T's hex to self's
   hex instead of from the player's, N from `min(A, 3)` down to 1, without
   setting the fleeing bit and without a taunt. Then run the attack loop.
8. **Weapon ranking (§3.9 preference rule, step 2).** The two weapons'
   primary subtypes are ranked by `best_weapon`; the higher rank wins, and
   equal ranks go to step 3 (cost). A null weapon still loses at step 1.

   | `best_weapon` | Ranged (4) | Throw (3) | Melee (2) | Unarmed (1) | None (0) |
   | --- | ---: | ---: | ---: | ---: | ---: |
   | `no_pref`, `random` | 3 | 2 | 1 | 1 | 1 |
   | `melee` | 1 | 1 | 2 | 1 | 1 |
   | `melee_over_ranged` | 2 | 1 | 3 | 1 | 1 |
   | `ranged_over_melee` | 3 | 1 | 2 | 1 | 1 |
   | `ranged` | 2 | 1 | 1 | 1 | 1 |
   | `unarmed` | 1 | 1 | 1 | 2 | 1 |
   | `unarmed_over_thrown` | 1 | 2 | 1 | 3 | 1 |

   The `no_pref` row reproduces SPEC-009 exactly. Under `unarmed`, §3.12 step 3
   (the unarmed upgrade) is skipped.
9. **Area attacks (§3.10).** After §3.10 selects hit mode M, when M's attack
   mode is burst or continuous (attack-mode nibble 7 or 8), the burst gate
   below decides. When the gate refuses and M is 3 while mode 2 is not burst
   or continuous, M becomes 2. When the gate refuses and mode 2 is also burst
   or continuous, the attack loop's next **attack** step returns failure
   instead of attacking.

   The **friendlies** are the player and the visible party critters, other than
   self and T, that are alive and on self's elevation. A friendly F is **at
   risk** when `dist(F, T) ≤ 1`, or F stands on, or next to, a hex that the
   SPEC-007 §3.2 ray from self's hex to T's hex visits strictly between them.

   | `area_attack_mode` | The gate allows a burst when |
   | --- | --- |
   | `always` | Always |
   | `sometimes` | At most 1 friendly is at risk |
   | `be_sure` | No friendly is at risk |
   | `be_careful` | No friendly is at risk, and none is within 3 hexes of T |
   | `be_absolutely_sure` | No friendly is at risk, and none is within 5 hexes of T |

Flee movement (§3.8), reloading (§3.13), join and stop predicates (§3.14),
team switching, retargeting and taunts are unchanged. `[Must-match]`

### 3.17 Party-Member Level-Ups

SPEC-018 §4.5's party table gives a member a minimum level L0
(`level_minimum`), an interval E (`level_up_every`) and an ordered list of n
level PIDs (`level_pids`). A member whose E is 0 never advances (SPEC-018
§4.5). `[Team A default]` for the algorithm.

- Immediately after each single player level gain (SPEC-011 §3.20), visit the
  party list in order. For each critter member, not hidden and not dead, that
  has a control record, n above 0 and E above 0: let s be its level stage.
  When `s < n` and the player's new level is at least `L0 + s × E`, the
  member
  advances one stage. A member advances at most one stage per player level
  gain.
- **Advancing:** s becomes s + 1. The member's prototype becomes level PID
  number s (1-based in the list), and its stats follow the new prototype.
  Current HP increases by (new maximum HP - old maximum HP), then is clamped to
  1 through the new maximum. The object's identifier, script, inventory,
  equipment, team, AI packet number, control record, position and facing are
  unchanged. When the new prototype's base art differs, the object takes it,
  keeping its facing and weapon code. The monitor prints E-level with the
  member's name.
- A PID that has no prototype stops the member at its current stage, with a
  diagnostic. `[Safe rule]`
- No random draw is made. `[Must-match]`

### 3.18 Mapping Onto the Enhanced Companion Mechanism

The enhanced Fallout 1 layer (roadmap part 2) has a Gear window and a Distance
toggle (Default, Close, Hold, Charge) read at SPEC-009 §3.6 step 4 and §3.8
step 1, and a Push entry. Fallout 2 defines its own behavior above. The two
relate as follows. `[Team A default]`

| Fallout 2 | Enhanced Fallout 1 | Relation |
| --- | --- | --- |
| Trade (section 3.7) | Gear (loot window, no steal roll) | Same purpose. Fallout 2 uses the barter window with a forced accept, and hides equipped items exactly as the Gear window does |
| `distance` = `on_your_own` | Default | Identical at both read sites |
| `distance` = `charge` | Charge | Identical: no pursuit stop, packet value at the flee stop |
| `distance` = `stay_close` | Close | Not identical: Close changes `max_dist` to 3; `stay_close` leaves `max_dist` alone and adds the leash and follow threshold |
| `distance` = `stay` | Hold | Not identical: Hold changes `max_dist` to 1; `stay` forbids movement |
| `distance` = `snipe` | None | No enhanced equivalent |
| Push (section 3.6) | Push | Same step rule (D, D + 1, D + 5). Fallout 2 adds the talk approach and `push_p_proc` |

Implementation guidance:

- The two read sites are shared code. The Fallout 2 `distance` field and the
  enhanced Distance mode are separate per-member values, and only the active
  profile's one is consulted.
- Under the Fallout 2 profile, the enhanced Gear and Distance entries are not
  added, whatever the preset, because Fallout 2 has its own. Under the
  Fallout 1 profile, nothing in this spec applies.

### 3.19 The Car

#### 3.19.1 State and Acquisition

The car is the state of section 2.4: `has_car`, `car_area`, `car_map` and
`car_fuel`, saved in SPEC-018 §7.4. `[Must-match]` that this state exists and
is saved.

- **Given to the party.** A script gives the car with `metarule` rule 31
  (SPEC-017 §5.1). Then `has_car` = 1, `car_area` = the city of the current
  map (SPEC-018 §5.2, -1 when none) and `car_map` = the current map's number.
  Fuel is unchanged. Giving it again moves it the same way.
- **Fuel given.** `metarule` rule 32 adds its signed operand:
  `car_fuel = clamp(car_fuel + n, 0, 80000)`. Without a car, nothing
  changes. `[Must-match]` for the maximum; `[Safe rule]` for the clamp.
- **Queries.** `metarule` rule 30, "car's current town", returns `car_area`
  when `has_car` is 1 and -1 otherwise. `metarule3` 110, "car out of fuel",
  returns 1 when `has_car` is 1 and `car_fuel` is 0, else 0.
- **Car object on a local map.** The engine never creates, places or removes a
  car object. The campaign's map scripts place the car and its trunk on a
  city's map when the "car's current town" query names that city.
  `[Must-match]`

#### 3.19.2 Driving or Walking

The party drives when the world map opens and all hold: `has_car` is 1, and
the map just left is a map of `car_area` (when `car_area` is not -1), or is
`car_map` itself (when `car_area` is -1). Otherwise it walks, and the car stays
where it is. `[Team A default]`

- While driving, the car is with the party. On entering any map from the
  world map while driving (a city, a terrain drop or an encounter),
  `car_area` becomes that map's city (or -1) and `car_map` its number.
- While walking, entering a map changes nothing about the car.
- The driving state is decided again at every opening of the world map.

#### 3.19.3 Travel in the Car

While driving with `car_fuel` above 0, each travel iteration of section
3.12.5 instead makes `max(1, trunc(4 / d))` pixel steps (section 2.6), with d
the difficulty of the subtile the iteration starts on, and ignores the terrain
counter. Each step is tested against the walking mask as SPEC-008 §3.4; a
blocked step stops travel, and the iteration's remaining steps are not made.
The iteration charges Q ticks as on foot, and counts as one iteration for the
W-iteration healing and encounter schedule. `[Team A default]`

- **Fuel.** An iteration that made at least one step uses 100 fuel, clamped
  at 0. Team A knows no data that names a fuel-regulator global, so no
  regulator rule exists in this revision. `[Team A default]`
- **Running dry.** When an iteration leaves `car_fuel` at 0, travel stops at
  the party's current pixel, at rest, after that iteration. A large Done box
  (SPEC-014 §3.13) at screen (169, 116) shows E-fuel-out in key 32328, with
  `iisxxxx1`. `[Team A default]`
- **Driving with an empty tank.** The car stays with the party (section
  3.19.2 still holds), but travel uses the walking schedule and uses no fuel
  until fuel is given again. The party is never stranded. `[Team A default]`

#### 3.19.4 The Fuel Gauge

While the party drives, the world-map side panel shows a vertical gauge at
window (493, 138), 6 by 189: filled from the bottom with key 992 over
`trunc(189 × car_fuel / 80000)` rows, and key 0 above. It is redrawn on every
pass that redraws the view, and restored from the frame when the party walks.
`[Team A default]`

#### 3.19.5 The Trunk

The trunk is an ordinary container object that the campaign scripts add to
the party (section 3.14), so that its contents travel with the party. The
scripts show it beside the car on the car's map and hide it elsewhere. The
engine treats it as any hidden or non-critter party entry and has no other
trunk rule. `[Must-match]`

#### 3.19.6 Trunk Capacity

The trunk's capacity is its prototype's container size (SPEC-011 §3.14).
There is no separate carry amount, and no script word sets or reads one
(SPEC-017 §6.1). `[Team A default]`

### 3.20 Death, Endgame and Credits

- **Death and knockout.** As SPEC-014 §3.5. Missing narration files follow its
  3000-millisecond rule. `[Must-match]`
- **Endgame slideshow.** `endgame_slideshow` requests the slideshow as SPEC-014
  §3.30; setup, still slides, desert pans, subtitles and exit are as there.
  The slides come from SPEC-018 §4.6's `endgame.txt` instead of SPEC-014's
  fixed table: every record whose global equals its value is shown, in table
  order. A record with direction 1 or -1 is a desert pan (left to right for 1,
  right to left for -1); direction 0 is a still slide. Art and palette are as
  SPEC-018 §4.6 gives them, narration under `sound/speech/narrator/`, as
  SPEC-014. Without `endgame.txt`, no slide is shown. No global is written afterwards. `[Must-match]` for the
  presentation; `[Team A default]` for "every matching record".
- **Ending movie.** `endgame_movie` runs SPEC-014 §3.31 steps 1, 3, 5 and 6.
  Steps 2 and 4 are not run, because Fallout 2 has no `maybe` track and no walk
  movies. `[Team A default]`
- **Credits.** As SPEC-014 §3.32. `[Must-match]`
- There is no engine-side time limit or water countdown (section 2.7).
  `[Must-match]`

### 3.21 Movies

The Fallout 2 game-movie table replaces SPEC-012 §2.11's. Its 17 files, in
this order, are also SPEC-018 §6.4's, and the record is saved as SPEC-018 §7.3
order 22. Playback, fades,
subtitles and the played-record rule are SPEC-012 §3.12's. `[Must-match]` for
the files; `[Team A default]` for the flags.

| Index | File (under `art/cuts/`) | `play_gmovie` flags |
| ---: | --- | --- |
| 0 | `iplogo.mve` | `0x0B` |
| 1 | `intro.mve` | `0x0B` |
| 2 | `elder.mve` | `0x0B` |
| 3 | `vsuit.mve` | `0x0B` |
| 4 | `afailed.mve` | `0x0B` |
| 5 | `adestroy.mve` | `0x0B` |
| 6 | `car.mve` | `0x0B` |
| 7 | `cartucci.mve` | `0x0B` |
| 8 | `timeout.mve` | `0x0B` |
| 9 | `tanker.mve` | `0x0B` |
| 10 | `enclave.mve` | `0x0B` |
| 11 | `derrick.mve` | `0x0B` |
| 12 | `artimer1.mve` | `0x0B` |
| 13 | `artimer2.mve` | `0x0B` |
| 14 | `artimer3.mve` | `0x0B` |
| 15 | `artimer4.mve` | `0x0B` |
| 16 | `credits.mve` | `0x0B` |

- The played-movie record has 17 bytes, one per index. `[Must-match]`
- `play_gmovie` with an index outside 0-16 is a failed play (SPEC-012 §3.13
  safe rule). `[Safe rule]`
- SPEC-012 §3.13's `0x02` addition for indexes 3 and 4 does not apply.
  `[Team A default]`
- Engine callers: startup (section 3.2), the main-menu Intro button and
  attract mode (index 1, flag `0x08`, as SPEC-014), the new game (section
  3.4), and the Pip-Boy archives (section 3.10, flags `0x0B`). There are no
  engine-called water or explosion movies. `[Must-match]`

### 3.22 Random-Draw Summary

This specification adds exactly one draw site, unchanged from the Fallout 1
rule it reuses:

| Decision | Draws | Condition |
| --- | --- | --- |
| Town-list or city-click destination | SPEC-008 §3.7's four jitter draws | Travel to a city the party is not in |

The encounter check, selection, spawning and terrain-drop draws are SPEC-018
§5.6-§5.7's.

Push, trade, dispositions, the AI deltas, level-ups and the car make no
draws. `[Must-match]`

## 4. Synthetic Test Vectors

All vectors use synthetic data: invented objects, numbers and names. Hex
numbers are SPEC-003 hex indexes on a 200-by-200 grid.

### 4.1 Vector A: Bar Geometry

| Input | Expected |
| --- | --- |
| `iface.frm` height 100 | Bar window (0, 379) 640 by 100; every element as SPEC-014; map window 640 by 381; map clicks accepted at y ≤ 379; indicator window y = 358 |
| Height 99 | Bar window (0, 380) 640 by 99; Inventory button window (211, 40) = screen (211, 420); HP counter window (473, 39); map window 640 by 382; map clicks at y ≤ 380; indicator window y = 359 |
| Height 50 | Treated as 100, one diagnostic |

### 4.2 Vector B: Indicator Boxes

ADDICT, SNEAK and POISONED lit; LEVEL and RADIATED not. Boxes in order
ADDICT, SNEAK, POISONED. Box 0 at x = 0, 127 wide (left 3 pixels cut); box 1 at
x = 130 - 6 = 124; box 2 at x = 260 - 9 = 251. Window width 381. ADDICT and
POISONED labels key 31744, SNEAK key 992.

### 4.3 Vector C: Main Menu

| Input | Expected |
| --- | --- |
| Release over a nonzero up-image pixel at (40, 147) | Button 3 (rectangle (30, 142) 26 by 26): Options; `nmselec0` on press, `nmselec1` on release; the preferences screen opens without the options menu |
| Key `O` | Options, `nmselec1` only |
| Label "OPTIONS" 60 pixels wide in font 104 | Drawn at (96, 143) |
| `misc.msg` without ID 20 | No copyright line; one diagnostic |

### 4.4 Vector D: Run-Away Thresholds

Member: maximum HP 60, current HP 44, packet `min_hp` 10, `hurt_too_much`
`crippled_legs`, left leg crippled (`0x04`), fleeing bit clear.

| `run_away_mode` | Threshold | Forced flee? |
| --- | ---: | --- |
| `none` | 10 (packet) | Yes, through `hurt_too_much` |
| `bleeding` | `trunc(60 × 75 / 100)` = 45 | Yes (44 < 45) |
| `not_feeling_good` | 30 | Yes, through `hurt_too_much` |
| `never` | Off | No: both tests disabled |

With the leg not crippled: `none` no, `bleeding` yes, `not_feeling_good` no,
`never` no.

### 4.5 Vector E: Chem Use

Member: maximum HP 61, current HP 40, AP 6, inventory two Stimpaks (PID 40),
each healing 10 HP.

| `chem_use` | B | Result |
| --- | ---: | --- |
| `clean` | | No healing; AP 6 |
| `anytime` | 30 | No healing (40 is not below 30) |
| `stims_when_hurt_little` | 45 | First Stimpak: HP 50, AP 4. Loop stops (50 is not below 45) |
| `always` with HP 20 | 45 | First: HP 30, AP 4. Second: HP 40, AP 2. No drug left |
| `sometimes` with HP 20 | 30 | First: HP 30, AP 4. Per-turn limit 1 reached |

### 4.6 Vector F: Attack Who

Self S, a party member on team 0. After sorting the snapshot from S:

| Entry | dist from S | Dead | Perceived by S | Last attacker | HP | Rating |
| --- | ---: | --- | --- | --- | ---: | ---: |
| A | 2 | No | Yes | Party member M | 10 | 12 |
| B | 4 | No | Yes | Player | 30 | 25 |
| C | 6 | No | No | Player | 5 | 30 |
| D | 7 | Yes | Yes | Player | 0 | 0 |

Hostile candidates, in order: A, B. C is not perceived; D is dead.

| `attack_who` | T |
| --- | --- |
| `whomever_attacking_me` | B |
| `closest` | A |
| `strongest` | B (25 > 12) |
| `weakest` | A (10 < 30) |
| `whomever` | `danger(S)` of SPEC-009 §3.5 |

### 4.7 Vector G: Weapon Ranking

Inventory, top level, in order: knife (melee, cost 40), pistol (ranged, cost
250, loaded), throwing knife (throw, cost 50). The right hand is empty, AP
suffice and all three are usable.

| `best_weapon` | Fold | Result |
| --- | --- | --- |
| `no_pref` | knife; pistol (3 > 1); throwing knife (2 < 3) | Pistol |
| `melee` | knife; pistol (1 < 2); throwing knife (1 < 2) | Knife |
| `ranged` | knife; pistol (2 > 1); throwing knife (1 < 2) | Pistol |
| `unarmed_over_thrown` | knife; pistol (1 = 1, 250 > 40, cost wins); throwing knife (2 > 1) | Throwing knife |
| `melee_over_ranged` | knife; pistol (2 < 3); throwing knife (1 < 3) | Knife |

### 4.8 Vector H: Area Attacks

Self at hex 20100 holds an SMG: primary single (nibble 6), secondary burst
(nibble 7). Target T at dist 5. Friendly F stands next to the third hex of the
SPEC-007 §3.2 ray from self to T, and 3 hexes from T. §3.10 selected M = 3.

| `area_attack_mode` | At risk | Gate | Mode used |
| --- | ---: | --- | --- |
| `always` | 1 | Allows | 3 (burst) |
| `sometimes` | 1 | Allows | 3 |
| `be_sure` | 1 | Refuses | 2 (single) |
| `be_careful` | 1 | Refuses | 2 |

With a minigun whose primary mode is also burst and `be_sure`: no attack, and
the attack loop returns failure at its attack step.

### 4.9 Vector I: Distance

No requested target. Player at dist 7 from self S, an enemy T at dist 4 from S,
`dist(T, player)` 9. Range-aware chance below `min_to_hit`, no-range chance
above it (SPEC-009 §3.12 row 0: approach).

| `distance` | Result |
| --- | --- |
| `on_your_own` | Approach T |
| `stay_close` | dist(S, player) 7 > 5: approach the player instead, no taunt; loop returns failure |
| `stay` | Approach fails; no flee; loop returns failure |
| `charge` | Approach T; §3.6 step 4 skipped afterwards |

Follow with no target at all and dist(S, player) 4: `stay_close` approaches the
player (4 > 3); `on_your_own` does not (4 ≤ 5); `stay` never does.

### 4.10 Vector J: Dispositions and the Custom Panel

Member allowed sets: disposition {`custom`, `defensive`, `aggressive`};
distance {`stay_close`, `on_your_own`, `charge`}. Record starts neutral.

| Action | Result |
| --- | --- |
| Choose row "Defensive" | Disposition `defensive`; fields `be_sure`, `bleeding`, `ranged_over_melee`, `stay_close`, `whomever_attacking_me`, `stims_when_hurt_little` |
| Press key `2` (Coward) | Row disabled: nothing changes |
| Custom panel, press the Distance button | `stay_close` → `charge` (next in section 2.5 order within the allowed set); disposition becomes `custom` |
| Press it again | `charge` → `on_your_own` |
| Press it again | `on_your_own` → `stay_close` (wraps) |

### 4.11 Vector K: Level-Ups

Party record: L0 = 3, E = 3, level PIDs [P1, P2, P3].

| Event | Stage before | Required | Stage after |
| --- | ---: | ---: | ---: |
| Player gains level 3 | 0 | 3 | 1 (PID P1) |
| Level 4 | 1 | 6 | 1 |
| Level 6 | 1 | 6 | 2 (PID P2) |

A member who joins at player level 12 with stage 0: level 13 → stage 1, level
14 → stage 2, level 15 → stage 3, level 16 → stays 3 (n = 3).

HP on advancing: old maximum 40, current 30, new maximum 48: current 38. Old
maximum 40, current 1, new maximum 35: current `clamp(1 - 5, 1, 35)` = 1.

### 4.12 Vector L: Car Fuel and Speed

| Input | Expected |
| --- | --- |
| Fuel 250, no regulator, driving on difficulty 1 | Iterations 1, 2, 3 make 4 steps each; fuel 150, 50, then 0; travel stops after iteration 3 with E-fuel-out |
| Fuel 250, driving on difficulty 1, a script raising the fuel by 100 after iteration 1 | After iteration 1: 150, raised to 250; after iterations 2, 3 and 4: 150, 50 and 0; travel stops after iteration 4 |
| Difficulty 2 / 3 / 5 | 2 / 1 / 1 steps per car iteration |
| Fuel 0, driving | Walking schedule; no fuel used; gauge empty |
| Fuel 100, give gas 90000 | 80000 |
| Fuel 300, give gas -500 | 0 |
| Gauge with fuel 40000 | `trunc(189 × 40000 / 80000)` = 94 rows filled from the bottom |

### 4.13 Vector M: Where the Car Is

Synthetic map table: maps 20 and 21 in city 4, map 30 in city 7, map 40 in
city 9, map 90 in no city. The car was given on map 20.

| Step | `car_area`, `car_map` | Driving at the next world-map opening? |
| --- | --- | --- |
| Given on map 20 | 4, 20 | |
| Leave map 21 to the world map | 4, 20 | Yes (map 21 is in city 4) |
| Enter map 30 while driving | 7, 30 | |
| A script loads map 40; leave it | 7, 30 | No: walking, no gauge |
| Walk into map 30 again, then leave it | 7, 30 | Yes |
| Drive into encounter map 90 | -1, 90 | |
| Leave map 90 | -1, 90 | Yes (it is `car_map`) |

"Car's current town" returns 7 after row 3 and -1 after row 6; with `has_car`
0 it returns -1.

### 4.14 Vector N: City Containment and Town Map Entry

City c at world (500, 400), size Medium (r = 13).

| Party position | Inside? |
| --- | --- |
| (510, 408) | Yes: 100 + 64 = 164 ≤ 169 |
| (511, 408) | No: 121 + 64 = 185 > 169 |

Entrances of c: entrance 0 known, map 30, tile -1; entrance 1 known, map 30,
tile 12345, elevation 1, orientation 2; entrance 2 known, map 30, tile 500,
elevation -1, orientation -1.

| Choice | Placement |
| --- | --- |
| Entrance 0 | Map 30's header default entrance; no campaign global written |
| Entrance 1 | Tile 12345, elevation 1, facing 2 |
| Entrance 2 | Tile 500, with the header's elevation and orientation |

An entrance line with orientation 8 is dropped when `city.txt` is parsed
(SPEC-018 §4.3), so it never gets a marker.

### 4.15 Vector O: Party Counts and Push

Party list: player; critter S (visible); trunk container T (hidden flag set).
All entries: 3. Visible entries, used only internally (section 3.14): 2.
`metarule` rule 16 returns 3. With
effective Charisma 7 the documented script rule allows `trunc(7 / 2)` = 3
followers, and 4 with Magnetic Personality; the engine refuses none.

Push: the player at hex P; member S adjacent in direction D = 1. S's neighbor in
direction 1 is blocked and its neighbor in direction 2 is free: S walks to the
direction-2 neighbor. With directions 1, 2 and 0 all blocked: the monitor
prints "There is no room to push S." and nothing moves. No draw in either case.

### 4.16 Vector P: Movies and Archives

Played bytes set for indexes 1, 2, 5 and 12. The Archives list shows 2, 5 and
12 (index 1 is outside 2-15), titled `pipboy.msg` 502, 505 and 512. Selection
1 plays index 2. `play_gmovie(17)` is a failed play with a diagnostic.

### 4.17 Vector Q: Endgame Slides

Synthetic slide table, in order: (global 5 = 1, still `endA`), (global 5 = 2,
still `endB`), (global 9 = 0, pan -1, `dp`). With global 5 = 2 and global 9 = 0:
`endB` as a still slide, then `dp` panned right to left. No global is written
afterwards.

### 4.18 Vector R: Premade Portraits (Revision 3)

A one-color `pickchar.frm` (640 by 480, index 9) and three one-color
portraits (indices 40, 41, 42), the first two 592 by 260 and the third 640 by
300. Each premade's screen shows:

- its own color at (27, 23), (618, 23), (27, 281) and (618, 281). These are
  art rows 0 and 258, both even;
- the background at:
  - (0, 0);
  - (27, 24) and (27, 282), art rows 1 and 259, which are odd;
  - (26, 23), (619, 23), (27, 22) and (27, 283), just outside the window.

The third portrait is clipped at the window's right and bottom edges. Its
art reaches (619, 23) and (27, 283), at column 592 and row 260, the first
column and row outside the window. A drawing without the clip would color
both.

Under Fallout 1, with three one-color 212-by-187 portraits, each screen
shows:

- its own color at (44, 53), (255, 53), (44, 239) and (255, 239);
- the background at (44, 54), an odd row, and at (43, 53), (256, 53),
  (44, 52) and (44, 240).

## 5. Decisions

Every point the brief and the public documentation leave open is resolved
here. None is an owner instruction except D-1.

- **D-1 (owner instruction, relayed in the coordinator's brief).** Scope and
  boundaries: this spec owns behavior; data files and the save layout are
  SPEC-018's; host words are SPEC-017's.
- **D-2 (Team A default).** Art is named, not indexed, and resolved through
  `intrface.lst` by name with stand-ins (section 2.2). The candidate names are
  Team A's best knowledge. The asset audit should list which section 2.2 names
  the local corpus lacks, so that a later revision can correct them.
  *(Revision 2: the retail audit resolves all 12 roles, and section 2.2
  records the names and sizes. Only the custom panel's size changes
  this spec.)*
- **D-3 (Team A default).** Missing messages fall back to English engine
  strings; the main-menu label IDs 9-14 and copyright ID 20, and the indicator
  IDs 102-104, are defaults.
- **D-4 (Team A default).** Main-menu geometry (buttons on the left at x = 30,
  labels centered on x = 126), the version text, and the menu music track.
- **D-5 (Team A default).** No premade ID text.
- **D-6 (Team A default).** *(Revised by D-31.)* New game: movie
  `elder.mve`, start map `maps/artemple.map`, globals from
  `data/vault13.gam`, all as SPEC-018 §6 gives them. The first revision's
  refusal when no calendar exists is gone, since SPEC-018 §6.1 always gives
  one.
- **D-7 (Team A default).** The bar keeps SPEC-014's screen positions, derived
  from the height of `iface.frm`.
- **D-8 (Team A default).** Five indicator boxes in the order ADDICT, SNEAK,
  LEVEL, POISONED, RADIATED, lit at any nonzero poison or radiation level.
- **D-9 (Team A default).** Push: party members only, out of combat, with the
  talk approach and the D, D + 1, D + 5 order of the enhanced Push.
- **D-10 (Team A default).** Inventory weight line shows the carry limit.
- **D-11 (Team A default).** The loot Take All button reuses the enhanced
  button's rectangle.
- **D-12 (Team A default).** Trade with a party member always accepts, and
  shows `inventry.msg` 27.
- **D-13 (Team A default).** The Combat Control button's place, art and `c`
  key; the control and custom panel layouts, keys and the cycling custom
  buttons.
- **D-14 (Team A default).** Disposition presets, and that presets ignore the
  allowed sets.
- **D-15 (Team A default).** The control record is per member, kept across
  `party_remove`, and initialized from the save, then the packet's keys, then
  the neutral record. Changing settings never edits the shared `ai.txt`
  packet table.
- **D-16 (Team A default).** Every SPEC-009 delta of section 3.16: the
  run-away fractions, the chem thresholds and per-turn limit, the hostile
  candidate set and the `attack_who` choices, the follow thresholds, the
  stay-close leash, `stay` without movement, the snipe retreat, the weapon
  ranking table, and the burst gate. All are draw-free and reduce to SPEC-009
  under the neutral record.
- **D-17 (Team A default).** Level-ups: deterministic, one stage per player
  level gain, with the HP rule of section 3.17.
- **D-18 (Team A default).** Party capacity 32, and dead members removed when
  the player leaves the map.
- **D-19 (Team A default).** Enhanced Gear and Distance are not offered under
  the Fallout 2 profile.
- **D-20 (Team A default).** World map: city containment by radius, circle
  and label drawing, locked cities, the side-panel geometry, the dial, the
  town-list scrolling, and the direct world-map move after a scripted
  transition. *(Revised by D-31: the terrain schedule is SPEC-018 §5.4's,
  and the encounter fallback is gone, since SPEC-018 §5.6 defines the
  check.)*
- **D-21 (Team A default).** Town-map entrance labels and direct
  tile/elevation/orientation placement, writing no campaign global.
- **D-22 (Team A default).** Car speed `max(1, trunc(4 / d))` steps per
  iteration; fuel use 100 per moving iteration, with no regulator rule;
  driving decided by `car_area`/`car_map` at each world-map opening; the
  empty-tank rule that never strands the party; and the gauge geometry.
  *(Revised by D-28: there is no `car_carry`.)*
- **D-23 (Team A default).** Endgame: every matching record shown in table
  order; the ending movie skips the walk movie and the `maybe` track.
- **D-24 (Team A default).** Movie flags `0x0B` for every index, and the
  Archives range 2-15.
- **D-25 (Team A default).** Fallout 1 campaign hooks that read fixed globals
  are off under Fallout 2 (section 2.7). Running them would corrupt Fallout 2
  campaign state.
- **D-26 (Team A default).** Encounter selection and population are left to
  SPEC-018 §5.6-§5.7, because they cannot be separated from the
  encounter-table grammar. *(The first revision's fallback is removed by
  D-31.)*

**Reconciliation decisions** (2026-10-25). These resolve conflicts between
SPEC-017, SPEC-018 and SPEC-019, are written the same way in each spec they
touch, and are listed in SPEC-018 §10 as R1-R6.

- **D-27 (Team A default, reconciliation 2026-10-25; SPEC-018 R1).**
  `load_map`'s entrance parameter, like every Fallout 1 write of campaign
  indexes 32 and 65, is stored in no campaign global (section 2.7, SPEC-017
  §3.1, SPEC-018 §5.1).
- **D-28 (Team A default, reconciliation 2026-10-25; SPEC-018 R2).** No
  script word exists for the visible party count or a car carry amount. The
  visible count is internal (section 3.14), and `car_carry` is removed, so
  the car has four saved values.
- **D-29 (Team A default, reconciliation 2026-10-25; SPEC-018 R3).**
  Elevators are SPEC-018 §4.9's engine-owned table, with SPEC-013 §3.21's
  behavior; they need no rule here.
- **D-30 (Team A default, reconciliation 2026-10-25; SPEC-018 R4).** Play is
  available while Fallout 2 scripts are gated, as SPEC-017 §8.7 defines
  (section 3.4); only script execution waits for the audit.
- **D-31 (Team A default, reconciliation 2026-10-25; SPEC-018 R5, R6).**
  City means SPEC-018's area. The new-game data, terrain schedule,
  encounter rules, quest and holodisk rules, display and location names, art
  row indexes, endgame and karma tables are SPEC-018's (sections 3.4, 3.9,
  3.10, 3.11, 3.12, 3.13, 3.20). A level-up interval of 0 means never. A
  Fallout 1 or otherwise incompatible slot shows the old-version state.
  `pipboy_active=No` disables the Pip-Boy. Kill types 0-18 are named from
  `proto.msg` 1450 + t, with engine-string fallbacks. The town-list offset
  is not saved.
- **D-32 (Team A, Revision 3, 2026-10-26, measured).** This decision was
  drafted by the coordinator. Team A re-measured it independently and took
  it over.
  - **Placement.** The premade portrait's top-left is the monitor window's,
    screen (27, 23). The window is where the retail portraits' bezel edge
    best fits `pickchar.frm`'s, measured on the owner's install (section
    2.2). The fit is unique within 40 pixels and is decisive about the
    offset, but it is not a pixel-for-pixel identity.
  - **Not observed.** No reference program was run or read, so the
    original program's placement was not observed. The placement is
    therefore a measured Team A default, not `[Must-match]`.
  - **Clipping.** The portrait is clipped to the 592-by-260 window
    (section 3.3), as a safe rule.
  - **Scan lines.** The scan-line effect is kept from SPEC-014 §3.7,
    because nothing measured says Fallout 2 drops it.

## 6. Coordination With SPEC-017 and SPEC-018

The first revision's requests were answered in the reconciliation pass of
2026-10-25 (D-27 to D-31). Where each now stands:

**From SPEC-017:**

1. **Party counts:** `metarule` rule 16 returns all entries. No word returns
   the visible count, which stays internal (section 3.14).
2. **Car:** `metarule` 31 gives the car, 32 gives signed fuel, 30 returns the
   car's current town, and `metarule3` 110 tests for an empty tank (section
   3.19.1). There is no carry amount (section 3.19.6).
3. **World map:** `mark_area_known` type 0 sets a city's known state,
   `metarule3` 104 sets entrance states, `metarule` 44 and 45 read the world
   position, and rule 46 the current city. `wm_area_set_pos` moves a city.
4. **`push_p_proc`** is host event 24, found by name (SPEC-017 §2.4 and
   §6.1).
5. **Control records:** no word writes them. `metarule3` 109 reads
   `chem_use` as section 2.5's panel position.
6. **The script gate:** SPEC-017 §8.7 defines Play while gated (section 3.4).

**From SPEC-018:**

1. **Saves:** the party list, control records with their level stage and
   retained records (SPEC-018 §7.5); the four car values (SPEC-018 §7.4);
   the 17-byte played-movie record (SPEC-018 §7.3 order 22). The town-list
   offset is not saved.
2. **Party table:** PID, allowed sets, `level_minimum`, `level_up_every` and
   `level_pids` (SPEC-018 §4.5). There is no per-disposition packet number.
3. **AI packets:** the Fallout 2 control keys are not read (SPEC-018 §4.7).
4. **Map table:** file names, city (SPEC-018 §5.2), location names (SPEC-018
   §5.2), music, `pipboy_active` (SPEC-018 §4.2).
5. **World map:** tiles, art rows, terrain and difficulty, fill, encounter
   percentages, selection, spawning and dropping to terrain (SPEC-018 §4.4,
   §5.3-§5.7).
6. **City table:** position, size, known, visited and lock states, art rows,
   display name and entrances (SPEC-018 §4.3, §5.8).
7. **New-game data:** SPEC-018 §6.
8. **Interface tables:** quests and holodisks, `genrep.txt`, `karmavar.txt`
   and `endgame.txt` (SPEC-018 §4.6); kill types 0-18 (SPEC-018 §7.3). There
   are no town reputations and no fuel-regulator global in this revision.
9. **Elevators:** SPEC-018 §4.9.

## 7. Clean-Room Verification Checklist

- [x] No C/C++ code, struct definitions, internal function or variable names of
  any reference program.
- [x] No source-tree file paths of any program, and no 40-character hex
  strings.
- [x] Only public SSL procedure and opcode names (`party_add`,
  `party_remove`, `metarule`, `play_gmovie`, `gdialog_barter`,
  `push_p_proc`, `endgame_slideshow`, `endgame_movie`) and public data-file
  key literals (`ai.txt` and `party.txt` keys and value tokens).
- [x] Every rule tagged `[Must-match]`, `[May-improve]`, `[Safe rule]` or
  `[Team A default]`, and every Team A default listed in section 5.
- [x] Test vectors use synthetic data only.
- [x] No decompilation, disassembly, re-implementation source or game asset
  was consulted (Provenance). Revisions 2 and 3 read frame sizes, and
  Revision 3 compared border pixels, on the owner's retail install; no art
  is reproduced.
- [x] Revision 3 was re-measured by Team A on 2026-10-26, independently of
  the coordinator's draft and without reading the engine's code. The
  sizes, the fit search of section 2.2, and Fallout 1's 212-by-187
  portraits are Team A's own measurements. Vector R's points were checked
  by hand against the window bounds and the odd-row rule. The ingestion
  patterns were rechecked by search: no source-tree path, no 40-character hex
  string and no code listing.
