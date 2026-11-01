# SPEC-024c: Mod Premade Sheets, Startup Overrides, and Table Parsing

**Status:** research hand-off, Revision 1, 2026-10-31. It needs ingestion. It amends SPEC-018, SPEC-019, and SPEC-024b as section 5 lists, and it changes no word, hook, or save format.

**Provenance:** written on the research side from *Fallout Et Tu* 2.0.3911's own data (its character sheets, `ddraw.ini`, `Maps.txt`, `CITY.TXT`, and script source), the option comments and notes it bundles from sfall v4.5.1 (`ddraw.ini`, `sfall-readme.txt`), *Restoration Project Updated* 2.4.34's tables as a comparison, and the approved specs of both repositories, read only. sfall's implementation code was **not** read, and nothing here was checked against a running sfall or the engine. During inspection one name search of the shipped `ddraw.dll` returned option names and nothing else. No rule rests on it, and it was not repeated.

**Oracle:** `oracle/SPEC-024c/` holds the vectors, the fixtures, and the measurements. Section 6 says how they relate to this text.

## Revision history

- **Revision 1 (2026-10-31).** Research hand-off, from the real-mod audit the owner reported the same day.

## Clean-Room Rules for Research Agents

- Document only binary layouts, formulas, state rules, and observable behavior.
- Do not copy code, struct definitions, function signatures, or internal variable names from `sfall`, `fallout1-re`, any decompilation, or any re-implementation.
- Public option names, public file names, and public script names are permitted. They are what mod files contain.
- Provide synthetic test vectors. Mod data is measured and never reproduced as bytes.

## 1. Overview & Scope

The real-mod audit of *Fallout Et Tu* passed every script and 5,000 headless ticks, and left three format gaps. This specification closes them. It has three parts:

- **Section 2:** the 440-byte `.GCD` character sheet.
- **Section 3:** the `ddraw.ini` keys that change how a session starts.
- **Section 4:** the `maps.txt` and `city.txt` rules that lost three entrances.

**What the audit's three gaps turned out to be.**

| Reported | Measured cause |
| --- | --- |
| 440-byte sheets | The 432-byte layout, then two more words. The last word is an appearance style that a script reads |
| The engine always starts on `artemple` | `StartingMap=V13ent.map`, in section `[Misc]`, with date, position, movie, and model keys beside it |
| 37 diagnostics and 3 dropped entrances | One cause gives 30 of the 37 and all 3 drops: section headers followed by text, which SPEC-018 does not accept. The other 7 are two harmless forms that retail data evidently holds too |

**The third gap is worse than dropped entrances.** Three headers in Et Tu's `Maps.txt`, for maps 76 to 78, carry a note after the `]`. SPEC-018 does not read them as headers, so the keys under them fall into the section before, map 75. Map 75 ends up with the keys of map 78: stem `DETHCLAW` instead of `JUNKDEMO`. So three records are lost and a fourth is silently wrong, and only the three entrances that named lost lookup names were visible. Section 4.1 fixes all of this with one rule.

Out of scope: every other `ddraw.ini` key, hero appearance art (SPEC-024b Decision D-42), and language folders under `premade/`.

**Invariants.** With sfall mode off, or with no `ddraw.ini`, section 3 changes nothing. Sections 2 and 4 change only what a file that SPEC-018 refused or misread now gives. The retail tables keep every record they have today. `[Must-match]`

### 1.1 Tags

| Tag | Meaning here |
| --- | --- |
| `[Must-match]` | Published text states it, or mod data measured here needs it |
| `[May-improve]` | A known defect, with a safe replacement |
| `[Safe rule]` | Nothing published settles it; the rule is this vault's definition. It is deterministic and never undefined |

### 1.2 Terms

A **diagnostic** is SPEC-018 section 3.5's: it never stops play. **Install folder** is SPEC-018 section 2.1's. **Sfall mode** is SPEC-024b section 3.1's. **Et Tu** means *Fallout Et Tu* 2.0.3911 with the layers its `mods_order.txt` enables: `fo1_base`, `fo1_interface`, `Anim_ImprovedSMutantDeath`, and `fo1_maps_mountains`. `fo1_new_premades` is optional and off by default.

## 2. The 440-Byte Character Sheet

### 2.1 Which layout

A sheet is read by its exact length, under either profile and in either sfall mode. `[Must-match]` for 440; the rest is SPEC-018 section 6.3 unchanged.

| Bytes | Layout |
| ---: | --- |
| 428 | SPEC-006 section 2.2 |
| 432 | SPEC-018 section 6.3 |
| 440 | The 432-byte layout, then section 2.3's two words |

Any other length is refused before any player state changes, as SPEC-018 section 6.3 says. `[Safe rule]` File names are compared without case: Et Tu ships `BLANK.GCD` and `combat.GCD`. `[Must-match]`

One mod mixes all three. `fo1_base` holds a 428-byte `BLANK.GCD`, 432-byte `demo.gcd` and `player.gcd`, and 440-byte `combat.gcd`, `diplomat.gcd`, and `stealth.gcd`. So the length must be tested per file, never per mod or per profile. `[Must-match]`

### 2.2 Layout

All words are signed 32-bit, big-endian, as in SPEC-006. There is no magic and no version. `[Must-match]`

| Offset | Bytes | Field |
| --- | ---: | --- |
| `0x0000` | 368 | As SPEC-006 section 2.2: flags, 35 base stats, 35 bonus stats, 18 skills, body, experience reward, kill category |
| `0x0170` | 4 | Extra critter word, as SPEC-018 section 6.3 |
| `0x0174` | 32 | Player name, raw single-byte text |
| `0x0194` | 16 | Four tagged-skill indices, -1 unused |
| `0x01A4` | 8 | Two trait indices, -1 unused |
| `0x01AC` | 4 | Unspent character points |
| `0x01B0` | 4 | **Appearance race word** |
| `0x01B4` | 4 | **Appearance style word** |
| `0x01B8` | | End of file; 440 bytes |

So a 440-byte sheet is 8 bytes longer than Fallout 2's and 12 longer than Fallout 1's. The 12 are the extra critter word at `0x0170` and the two words here.

At these offsets every 440-byte sheet in Et Tu decodes to a named character with three distinct tags, two valid traits, and 0 points. `oracle/SPEC-024c/gcd-layout.tsv` gives this table beside the other two.

### 2.3 What the two words are

**The style word is measured.** `fo1_new_premades` replaces the three premades, and its global script gives each one starting items only when two things hold: the player's name is the premade's, and sfall global `HApStyle` equals that premade's style constant.

| Sheet | Name | Script's style constant | Word at `0x01B4` |
| --- | --- | ---: | ---: |
| `combat.GCD` | Ken | 1 | 1 |
| `stealth.GCD` | Sarah | 1 | 1 |
| `diplomat.GCD` | Louis | 2 | 2 |

All three agree, and the `fo1_base` sheets, which that script does not name, hold 0. So this word is the appearance style that the sheet gives the player. `[Must-match]`

This is inferred from the data and the script. No published text describes the word, and what sets `HApStyle` under sfall was not observed (Decision D-3).

**The race word is named by position only.** It is 0 in all six 440-byte sheets. Beside the style word, and with the public pair `set_hero_race` and `set_hero_style`, the name is a reasonable reading and no more. It has no effect here. `[Safe rule]` (Decision D-4)

### 2.4 Effect

When the player takes a 440-byte sheet in a new game, and sfall mode is on, and `EnableHeroAppearanceMod` in `ddraw.ini` is 1 or 2: `[Safe rule]`

1. Sfall global `HApStyle` becomes the style word. The key is exactly 8 bytes, as SPEC-024b section 4.9 requires.
2. The stored style of `set_hero_style` becomes the same number (SPEC-024b section 4.11). It changes no art.

Both happen after SPEC-018 section 6.6 installs the world state, and before session setup (SPEC-024b section 6.1), so `game_loaded` already sees them. The global is saved in the sidecar like any other, and is therefore in `sfall_hash`. `[Safe rule]`

Otherwise nothing is written. A 428-byte or 432-byte sheet has no such word, so it writes nothing, and `HApStyle` reads 0. The rest of the sheet loads as SPEC-006 section 2.3 says, whatever the two words hold. `[Must-match]`

Vectors A1 to A6 give each case. **Without this rule the premades load cleanly and `fo1_new_premades` silently gives them nothing**, because its test fails.

### 2.5 Other sheet rules

- **Keeping the words.** Each sheet keeps its length and its two words as read. They are not part of the player's state. `[Safe rule]`
- **Writing.** The character editor's SAVE (SPEC-014 section 3.43) writes 440 bytes, with the words as read, when the sheet in hand was read at 440 bytes. Otherwise it writes what SPEC-018 section 6.3 says. So a load and a save give identical bytes. `[Safe rule]` (Decision D-5)
- **Other sizes in one mod.** `BLANK.GCD` holds a nonzero body category and kill category. SPEC-006 section 2.3's loader forces them to zero, so no new rule is needed. `demo.gcd` and `player.gcd` hold a name of bytes above `0x7F`, which stay raw text. `[Must-match]`

### 2.6 What was measured

| Sheet | Bytes | Name | Style word |
| --- | ---: | --- | ---: |
| `fo1_base/premade/BLANK.GCD` | 428 | `None` | None |
| `fo1_base/premade/demo.gcd`, `player.gcd` | 432 | Three high bytes | None |
| `fo1_base/premade/combat.gcd` | 440 | `Max` | 0 |
| `fo1_base/premade/stealth.gcd` | 440 | `Natalia` | 0 |
| `fo1_base/premade/diplomat.gcd` | 440 | `Albert` | 0 |
| `fo1_new_premades/premade/combat.GCD` | 440 | `Ken` | 1 |
| `fo1_new_premades/premade/stealth.GCD` | 440 | `Sarah` | 1 |
| `fo1_new_premades/premade/diplomat.GCD` | 440 | `Louis` | 2 |

The word at `0x0170` and the race word are 0 in all of them. Elsewhere on the research machine, Restoration Project Updated's 12 sheets are 432 bytes and Fallout 1's 6 are 428, so 440 bytes is an Et Tu form here.

## 3. Startup Overrides from `ddraw.ini`

### 3.1 The file

- **When.** Under sfall mode only, once, when the session is built, before the main menu. `[Safe rule]` With sfall mode off the file is never opened. `[Must-match]`
- **Where.** By SPEC-024b section 4.5's lookup (the `Vfs` layers, then the writable folder), then the install folder, with the name compared without case. `[Safe rule]`
- **Missing.** No file gives every default below, with no diagnostic. `[Must-match]`
- **Which file.** Only `ddraw.ini`. Et Tu's `ddraw.fo1in2.ini` is an input to its own release build, which merges it into `ddraw.ini`, and nothing reads it at run time. The two agree on every key below except `VersionString`, where it holds a placeholder `?`. `[Must-match]` A session never reads an alternative name. `[Safe rule]`
- **Replays.** The file is a replay input, as SPEC-024b section 4.5 says of INI files. No input-log record is added (Decision D-7).

### 3.2 Grammar

SPEC-018 section 3.3's grammar, as section 4.1 amends it, with its rule 10 for unknown keys. Every key below lives in section `[Misc]`, and the same key in another section does not count. `[Must-match]`

**Unset.** A key is unset when it is absent, commented out with `;`, or empty. A numeric key is also unset at -1, the published "leave as default" value. An unset key takes its default with **no** diagnostic. `[Must-match]`

**Invalid.** A value of the wrong form takes the default, with one diagnostic, and never stops the session. `[Safe rule]`

### 3.3 The keys

| Key | Form | Default when unset or invalid | Et Tu's value | Section |
| --- | --- | --- | --- | --- |
| `StartingMap` | Stem | `artemple` | `V13ent.map` | 3.4 |
| `Movie1` to `Movie17` | File name | SPEC-018 section 6.4's row | 17 values | 3.5 |
| `StartYear` | Integer, 0 or more | 2241 | 2161 | 3.6 |
| `StartMonth` | Integer 0-11 | 6 | 11 | 3.6 |
| `StartDay` | Integer, from 0 | 24 | 4 | 3.6 |
| `StartTime` | Integer, `HHMM` | 721 | 721 | 3.6 |
| `StartXPos`, `StartYPos` | Integers | The start area's position | 823, 72 | 3.7 |
| `ViewXPos`, `ViewYPos` | Integers | SPEC-019's first view | 600, 0 | 3.7 |
| `MaleStartModel`, `FemaleStartModel` | `critters.lst` base name | The existing rule | `hmjmps`, `hfjmps` | 3.8 |
| `MaleDefaultModel`, `FemaleDefaultModel` | `critters.lst` base name | `hmjmps`, `hfjmps` | `hmjmps`, `hfjmps` | 3.8 |
| `VersionString` | Text, at most 63 bytes | `FALLOUT II 1.02d` | `FALLOUT ET TU v1.16.3903` | 3.9 |
| `MainMenuOffsetX`, `MainMenuOffsetY` | Integers | 0 | 392, 26 | 3.9 |
| `MainMenuCreditsOffsetX`, `MainMenuCreditsOffsetY` | Integers | 0 | Unset | 3.9 |
| `MainMenuFontColour` | Hexadecimal | SPEC-019's key | `0x000055` | 3.9 |
| `PremadePaths`, `PremadeFIDs` | Lists | SPEC-018 section 6.3's three | Unset | 3.10 |

`[Must-match]` for each name, section, published default, and Et Tu's use.

### 3.4 `StartingMap`

The value is a Stem (SPEC-018 section 3.4), so the final `.map` is optional, and `V13ent.map`, `V13ent`, and `V13ENT.MAP` are one value. `[Must-match]`

- **Set and valid.** The stem replaces `artemple` in all of SPEC-018 sections 6.5 and 6.6: the new-game map, the start area, the current entrance, and the city music. `[Must-match]` The map loads as there, by the header's default entrance. `maps.txt` needs no record for it, as that section already allows.
- **Unset.** `maps/artemple.map`, exactly as today, with no diagnostic. `[Must-match]`
- **Not a Stem, or `maps/<stem>.map` absent.** The test runs in step 1 of SPEC-018 section 6.6, before anything changes. The new game falls back to `artemple`, with a diagnostic naming the value. `[Safe rule]` (Decision D-6)

For Et Tu, map `V13ENT` is named by entrance 0 of area 0, so SPEC-018 section 6.5 gives area 0 as the start area, at `world_pos` 823,72. That equals `StartXPos` and `StartYPos`, so the two sources agree.

### 3.5 Movies

`MovieN` replaces the file of SPEC-018 section 6.4's index `N - 1`. The value is a file under `art/cuts/`, a Stem and `.mve`, and a path is invalid. `[Must-match]`

- Everything that uses an index uses the new file: startup (indexes 0 and 1), the new-game hand-off (index 2), and `play_gmovie`. So Et Tu's `Movie3=ovrintro.mve` replaces `elder.mve` in the hand-off. Flags stay with the index. `[Must-match]`
- A missing file fails the play as SPEC-012 section 3.12 step 1 says. `[Must-match]`
- The published `Movie18` to `Movie32` are not supported, because the played-movie record holds 17 bytes. Each set key gets a diagnostic. `[Safe rule]` Et Tu sets none. (Decision D-8)

### 3.6 Start date and time

These change SPEC-018 section 6.1 only. Published: month and day are 0-indexed, and `StartTime` is `HHMM` with the leading zero optional. `[Must-match]`

- **`StartTime`.** With `HH = trunc(value / 100)` and `MM = value mod 100`, valid when `HH <= 23` and `MM <= 59`, the reset clock is `(60 x HH + MM) x 600` ticks. So 721 gives 264600, today's value. `[Safe rule]` for the arithmetic.
- **`StartYear`, `StartMonth`, `StartDay`.** They give the date of game day 0, in place of 25 July 2241. Each key stands alone. `StartDay` is tested against the month chosen, so a day past its end is invalid. `[Safe rule]`
- **Calendar.** SPEC-018 section 6.1's non-leap calendar counts forward from that date. Et Tu's values give 5 December 2161 at 07:21. It holds for the whole session, and is not saved: the file gives it. `[Safe rule]`

`Fallout1Behavior` is not read (section 3.11). Its published note speaks of the initial time without giving a value, and Et Tu sets these keys itself.

### 3.7 World position

`StartXPos` and `StartYPos` are world pixels, and replace SPEC-018 section 6.5's world position. They apply as a pair. One alone, or a point outside the world (SPEC-018 section 5.3), is invalid, and the start area's position stands. `[Must-match]` for the override; `[Safe rule]` for the pair. The override is applied after the starting map's direct move of SPEC-018 section 6.6 step 3. `[Safe rule]`

`ViewXPos` and `ViewYPos` set the first world-map view, as a pair, and change only drawing. `[Must-match]`

### 3.8 Player models

- **`MaleDefaultModel`, `FemaleDefaultModel`** replace the base names `hmjmps` and `hfjmps` in SPEC-005's jumpsuit scan. `[Must-match]`
- **`MaleStartModel`, `FemaleStartModel`** name the player's art at a new game, by gender. Unset, the existing rule stands. `[Must-match]`
- A name that `critters.lst` does not hold is invalid. `[Safe rule]`

The bundled comments show `hmwarr` and `hfprim` as the start values. Et Tu sets all four to the jumpsuits, because its campaign begins in the Vault.

### 3.9 Title

- **`VersionString`** replaces SPEC-019's version text on the main menu, at the same place. Up to two `%d` are published. The first becomes 1 and the second 2. `[Safe rule]` for the numbers. More than two, or any other `%`, is invalid. `[Safe rule]`
- **`MainMenuOffsetX`, `MainMenuOffsetY`** are added to the positions of the six buttons and their labels. **`MainMenuCreditsOffsetX`, `MainMenuCreditsOffsetY`** are added to the copyright line's. `[Must-match]`
- **`MainMenuFontColour`.** The last byte is the palette index for the version text and the copyright line. `[Must-match]` The published "first byte" holds flags, read here as bits 16 to 23. Flag 1 limits the colour to the version text. Flags 2 and 4, underlining and a monospace font, are not supported, and get a diagnostic. `[Safe rule]` Et Tu's value has no flag.

### 3.10 Premade list

`PremadePaths` and `PremadeFIDs` replace the three premades, in order. Both must be set, with equal counts, and each name at most 11 bytes. Otherwise the default stands, with a diagnostic. `[Must-match]` Et Tu leaves both unset.

### 3.11 Keys that are not read

Et Tu's `ddraw.ini` holds 149 keys in `[Misc]`, and most are engine patches, which SPEC-024b section 1 leaves out of scope. These touch the start, and are deliberately not read: `[Safe rule]`

| Key | Et Tu's value | Why not |
| --- | --- | --- |
| `PipBoyAvailableAtGameStart` | 2 | SPEC-019 section 3.10 gates the Pip-Boy only by `pipboy_active`, so it is already available |
| `Fallout1Behavior` | 1 | Its published effects are on the end of the game |
| `DisableHorrigan` | 1 | A Fallout 2 encounter, not a start rule |
| `ConfigFile`, `PatchFile`, `[ExtraPatches]` | Unset | Mounting belongs to SPEC-018 and SPEC-024a |

## 4. `maps.txt` and `city.txt`

Four amendments to SPEC-018. Section 4.1 changes the grammar of all four table files. The others are narrow. Nothing here makes an invalid entrance valid: an entrance naming a map that no record has is still dropped.

### 4.1 Section headers

**SPEC-018 section 3.3 rule 5 becomes:** A line that begins with `[` and has a `]` after it opens a section. Its name is the text between the `[` and the first `]`, trimmed. Text after that `]` is ignored, with no diagnostic. `[Must-match]`

- **Evidence.** SPEC-009 section 2.2 records R1's own configuration reader: bytes after the `]` are ignored, and `#` is not a comment marker. Et Tu's three headers need exactly that.
- **`#` is still not a comment marker.** The note is ignored because it follows a header. On a pair line, a `#` stays part of the value.
- **A line with no `]`** is not a header, and rule 7 applies: it is ignored with a diagnostic, and the lines under it stay in the section before. `[Must-match]`
- **A `[` after the start of a line** opens nothing. R1 would treat `color=[x]` as a header, and this rule keeps SPEC-018's narrower form, so such a line stays a pair. `[Safe rule]` No line in either mod's `maps.txt`, `city.txt`, or `worldmap.txt` tells the two forms apart. (Decision D-9)

### 4.2 Entrances of eight items

**SPEC-018 section 4.3's 8-item rule gains:** when item 6, the hex, is -1, and item 8 is an Integer, the line takes **no** diagnostic. `[Safe rule]` Any other 8-item line keeps its diagnostic. It is still read from its first seven items.

A hex of -1 takes the header's whole default entrance and ignores items 5 and 7 (SPEC-018 section 4.3), so every way of reading such a line gives the same entrance. Et Tu has six of these (`entrance_0` of areas 53 to 58) and Restoration Project Updated two. SPEC-018's Decision R16 records the retail pair.

### 4.3 Ambient lists

**SPEC-018 section 4.2's `ambient_sfx` gains:** an item that holds spaces or tabs is split at them, and each part is read as an item. `[Safe rule]`

Both mods hold `animal:15 animal:10`, one missing comma that both evidently inherit from retail. The key has no effect in this revision, so nothing changes in play. A part that is still invalid is dropped, as today.

### 4.4 Records that share a map file

**SPEC-018 section 4.2's duplicate rule gains:** two records with equal stems and different `lookup_name`s are **aliases**, and take no diagnostic. `[Safe rule]` Two records with the same stem and the same lookup name, or without one, keep theirs.

Et Tu's maps 76, 77, and 78 are aliases of maps 16, 65, and 37, and its `Maps.txt` says they exist for world-map circles: areas 61 to 63 reach the same files by other lookup names. The rest of that rule stands: number-to-file lookups use the record's own stem, and name-to-number lookups give the lowest number. So loading through an alias gives the lower map number, and with it the same working `.SAV`, which is what one file needs. `[Must-match]`

### 4.5 Results

`oracle/SPEC-024c/table-measured.tsv` holds these. `Maps.txt` and `CITY.TXT` are read with the worldmap size of 1400 by 1800 pixels.

| Corpus | Rules | Maps | Areas | Entrances listed | Dropped | Diagnostics |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Et Tu | SPEC-018 as written | 90 | 65 | 99 | 3 | 37 |
| Et Tu | Section 4.1 only | 93 | 65 | 99 | **0** | 10 |
| Et Tu | All of section 4 | 93 | 65 | 99 | **0** | **0** |
| Restoration Project Updated | SPEC-018 as written | 173 | 61 | 185 | 0 | 3 |
| Restoration Project Updated | All of section 4 | 173 | 61 | 185 | 0 | **0** |

The first row reproduces the audit's 37 diagnostics and 3 dropped entrances exactly, by an independent reading of SPEC-018. Those 37 are 3 unread headers, the 23 repeated keys they cause, 1 false duplicate stem, 3 dropped entrances (areas 60, 61, and 62), 6 eight-item entrances, and 1 ambient item. The first four kinds, 30 in all, come from the headers. Section 4.1 alone leaves 10, not 7, because maps 76 to 78 then exist and add 3 alias diagnostics, which section 4.4 settles. Every one of Et Tu's 93 records has its map file in an enabled layer.

Of the 25 `.txt` tables in the two mods' data folders, only Et Tu's `Maps.txt` has a header followed by text, so section 4.1 changes no other table.

## 5. Amendments

| Spec | Section | Change |
| --- | --- | --- |
| SPEC-018 | 3.3 rule 5 | Section 4.1 |
| SPEC-018 | 4.2 | Sections 4.3 and 4.4 |
| SPEC-018 | 4.3 | Section 4.2 |
| SPEC-018 | 6.1 | Start date and time may come from `ddraw.ini` (section 3.6) |
| SPEC-018 | 6.3 | "Which layout": 440 bytes joins 428 and 432 (section 2), and SAVE keeps a 440-byte sheet's length |
| SPEC-018 | 6.4 | The table is the default for `Movie1` to `Movie17` (section 3.5) |
| SPEC-018 | 6.5, 6.6 | `artemple` is the default for `StartingMap`, and the world position may be overridden (sections 3.4 and 3.7) |
| SPEC-019 | Main menu | The version text and the positions are defaults (section 3.9) |
| SPEC-024b | 1 | The `ddraw.ini` keys of this spec are no longer out of scope |

Team A should carry these into those specs at ingestion. No word, hook, or sidecar byte changes. SPEC-024b's own `ExtraGameMsgFileList` and `BoxBarCount` readings of the file stand.

## 6. Fixtures

All data in the oracle is synthetic, except `table-measured.tsv`, which holds counts. `oracle/SPEC-024c/README.md` explains each file.

| File | Coverage |
| --- | --- |
| `gcd-layout.tsv` | The three layouts, side by side |
| `gcd-g1.hex` to `gcd-g5.hex` | Five complete synthetic sheets, as spaced bytes |
| `gcd-vectors.tsv` | G1 to G8: each sheet's decoding, and three refused lengths |
| `gcd-effect.tsv` | A1 to A6: section 2.4 |
| `ddraw-vectors.tsv` | D1 to D47: section 3, one key at a time |
| `fixture-ddraw.ini`, `fixture-ddraw-expected.tsv` | One whole startup block |
| `fixture-maps.txt`, `fixture-city.txt`, `table-vectors.tsv` | Section 4, under both rule sets |
| `table-measured.tsv` | Section 4.5's table |

### 6.1 Required tests

1. Each of G1 to G5 decodes as `gcd-vectors.tsv` says, and G6 to G8 are refused with no player state changed.
2. Loading G5 and saving it gives identical bytes. So do G1 and G2.
3. Each of A1 to A6 gives its global and stored style.
4. Every row of `ddraw-vectors.tsv`, with its diagnostic count. `fixture-ddraw.ini` gives `fixture-ddraw-expected.tsv`.
5. Sfall mode off with a `ddraw.ini` present reads nothing: every SPEC-021 golden is byte-identical.
6. `fixture-maps.txt` and `fixture-city.txt` give the `SPEC-024c` rows of `table-vectors.tsv`, with 6 maps, 2 areas, 1 dropped entrance, and 6 diagnostics.
7. `[Map 076] # note` opens map 76, and the keys under it do not reach map 75.
8. On Et Tu's tables, no entrance is dropped, there are no diagnostics, and map 75's stem is `JUNKDEMO`.
9. On the retail tables, every record and entrance is as before, and no diagnostic is new.
10. On Et Tu, a new game starts on `V13ENT` in area 0, on 5 December 2161 at 07:21, with the party at 823,72.
11. With `fo1_new_premades` enabled, taking each of its three premades gives `HApStyle` 1, 1, and 2, and the mod's script gives that character its items.

## 7. Clean-Room Verification Checklist

- [x] Every normative rule tagged `[Must-match]`, `[May-improve]`, or `[Safe rule]`
- [x] The 37 diagnostics and 3 dropped entrances reproduced from SPEC-018's text before any rule was changed
- [x] Restoration Project Updated's tables run as a regression
- [x] Vectors computed by a reference model, and regenerating them gives identical bytes
- [x] Spec linter run against `fallout1-re` symbols with 0 errors
- [x] The ingestion gate prints `ok`
- [x] No mod bytes reproduced: the oracle holds counts and synthetic data
- [ ] Rules checked in the engine: tests 8 to 11 need a real install
- [x] Owner approval of Decisions D-1 and D-3 (2026-10-31)

## 8. Decisions

Every decision is a research default unless the Class column says otherwise.

| ID | Decision | Class | Why |
| --- | --- | --- | --- |
| D-1 | Fix the table parser at the header rule, not by relaxing entrances | Owner approved (2026-10-31) | The brief asks for relaxed entrance rules, but the entrances are valid. They fail because map records are lost, and map 75 is silently wrong, which relaxing entrances would hide |
| D-2 | A sheet of 440 bytes is the 432-byte layout and two words, not a third layout | Default, from measurement | Name, tags, traits, and points decode at the same offsets as 432 bytes in all six sheets |
| D-3 | The word at `0x01B4` is the appearance style, and it sets `HApStyle` | Owner approved (2026-10-31), from script evidence | Three script constants match three sheet words. No published text describes it, and sfall's own step was not observed |
| D-4 | The word at `0x01B0` is named race and has no effect | Default | It is 0 everywhere, so nothing can test it |
| D-5 | SAVE keeps a 440-byte sheet at 440 bytes | Default | As SPEC-018 keeps its extra word: a round trip should change no byte |
| D-6 | A bad `StartingMap` falls back to `artemple` | Default | A start must not fail on one key. The value is checked before anything changes, so the fallback is clean |
| D-7 | A replay reads `ddraw.ini`, with no new log record | Default | SPEC-024b section 4.5 already makes INI files replay inputs, and a different file diverges at the first hash |
| D-8 | `Movie18` to `Movie32` are not supported | Default | The played-movie record is 17 bytes in the save format, and Et Tu sets none |
| D-9 | The header rule needs a `[` at the line's start | Default | R1's wider form makes a value such as `color=[x]` a header, and no data needs it |
| D-10 | The rule of section 4.1 covers all four table files | Default | They share one grammar, and it changes nothing in the other three |
