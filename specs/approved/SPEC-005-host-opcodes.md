# SPEC-005: Interpreter-Library and Game Host Opcodes

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from the inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This document specifies the host instruction registrations in reference revision R1, targeting the 32-bit Windows interpreter described in SPEC-004. The evidence is static inspection of instruction handlers and their host consumers, not measurement of an original executable. Public bytecode mnemonics below are catalog labels, not native implementation routine names or serialized strings.

The inclusive range `0x804C`-`0x8155` contains 266 instruction words: 83 registered general-library words, two unregistered holes, and 181 registered game words. Every word has a row in sections 2.3-2.10. A registered word can still be a stub, reject ordinary-looking operands, leave a result absent on an early return, or invoke defective host behavior.

Two requested facilities require scope corrections. External symbol registration/access belongs to core words `0x8014`-`0x8017`, outside this range; section 2.2 includes their actual contracts. Animation-registration end is command 3 of `0x810E`, still consuming two operands; `reg_anim_end` is not a separate registered instruction. `[Must-match]`

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: gameplay-visible behavior, data grammar, formula, ordering, or random-draw sequence. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, undefined behavior, uninitialized reads, stale state, presentation-only transients, or a lookup that reads the wrong table in an obviously unintended way. A clean-room engine may apply the stated safe rule. It should keep every other observable behavior. An engine that wants bit-exact parity may offer a compatibility switch that keeps the reference behavior.

In the opcode reference tables (sections 2.2-2.10), the pop order, push column, and operand type checks of every row are `[Must-match]`. Each row's last column carries its own tags for the host-effect text. A `[May-improve]` safe rule never changes the row's operand count or type checks unless it says so.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Encoding, Value Types, and Reference-Table Notation

| Instruction-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `+0` | 2 | Big-endian opcode word |
| `+2` | 0 | No inline operands for any extension in this document; arguments come from the value stack |

Typed constant pushes used in the fixtures instead have a two-byte big-endian tag/opcode followed by a four-byte big-endian payload. Integers are signed 32-bit two's-complement; floats are IEEE-754 binary32. A runtime stack value is six bytes: four-byte big-endian payload followed by two-byte big-endian tag. A pop removes the tag first, then its payload. Return-stack contents are not implicitly used as extension operands. `[Must-match]`

| Symbol | Meaning / type test |
| --- | --- |
| I | Integer base tag `0xC001`; object references and handles also use this tag |
| S | String base tag `0x9001`, accepting static `0x9001` and dynamic `0x9801` |
| F | Float base tag `0xA001` |
| D | Returned dynamic string, tag `0x9801`, owned by the calling program |
| U | One complete tagged value is popped, but its tag is not checked |
| O | Optional text: resolve S; use null text for every other tag without rejecting it |
| N | S, or integer zero explicitly mapped to a null filename |
| C | Fill-color rule: F; I zero; I one converted to float 1; reject other I; other tags' payloads are unchecked float bits |
| T | Text-color rule: F or I zero; reject nonzero I, including 1; other tags' payloads are unchecked float bits |
| none | No operands popped, or no value pushed, according to the column |

The symbol rules above are `[Must-match]`. Where the C and T rules accept another tag's payload as unchecked float bits, see section 3.1 for the safe rule.

Type checks compare the tag after clearing bit `0x0800`, using mask `0xF7FF`. This is not arbitrary numeric coercion: a normal float is not accepted as an integer. Noncanonical integer/float tags with qualifier bit `0x0800` can also pass these masked tests; do not silently normalize their raw tags before subsequent string resolution. `[Must-match]`

In every reference table, **Pop order is first popped to last popped**, not source-call argument order. Unmarked operands are I; an explicit suffix such as `text S` overrides that default. Push arguments in the reverse of the listed order. Return entries describe pushes, not an implicit native status result: a statement marked `none` must not leave an invented zero. `I bool` means an integer-tagged 0 or 1. Other I results retain their stated status/error values. `[Must-match]`

Opaque object references are process-local 32-bit host references; zero is null. They are not PIDs, unique object numbers, hex numbers, or portable file offsets. Integer validation does not establish pointer validity, ownership, object category, or bounds. Unless a row says otherwise, numeric ranges are not checked by the opcode itself. Diagnostic-only null checks are distinguished from VM errors and unchecked null dereferences. `[Must-match]` Unchecked null or invalid references are reference defects; each affected row states its own safe rule. `[May-improve]`: an engine may reject an invalid nonzero reference with a diagnostic and the row's null result.

VM-error type checks and host-error checks can occur after some or all operands have already been consumed. Normal-path pop lists are not a promise that a rejected call first drains every operand. A checked type failure raises the interpreter's error, ends the program, and sets error state; it is not a returned -1. A host diagnostic followed by an ordinary return does not by itself raise that VM error. `[Must-match]`

### 2.2 External Symbols: Actual Core Words Outside the Extension Range

| Word | Public mnemonic | Pop order | Push | Checks and observable behavior |
| --- | --- | --- | --- | --- |
| `0x8014` | `fetch_external` | identifier offset U | Stored tagged value | Resolve the identifier-table name; missing variable is a VM error. Exported text is copied into this program's dynamic-string storage, not shared as an offset. `[Must-match]` |
| `0x8015` | `store_external` | identifier offset U, value U | none | Missing name is a VM error. Replace exported value/tag; string characters are copied to registry-owned storage. `[Must-match]` |
| `0x8016` | `export_variable` | identifier offset U | none | Register/reset a case-insensitive exported variable to I zero; duplicate owned by another program filename, or full registry, is a VM error. The owner filename is the path the program was loaded from (script directory plus its `scripts.lst` file name), compared case-insensitively, so every instance of the same `.INT` is the same owner. The same owner filename can reset it: the value becomes I zero and a held string is released. `[Must-match]` |
| `0x8017` | `export_procedure` | procedure-table index U, argument count U | none | Obtain name and entry address from that procedure record; register its address and supplied argument count. Ownership is by **program instance**, not filename: the owning program may re-register its own name, but any other program owning the name, including another loaded instance of the same `.INT`, is a VM error, as is a full registry. Neither operand tag is checked. `[Must-match]` |

Procedure and variable registries are separate, each with 1013 entries and stored names of at most 31 characters. Names are matched case-insensitively. `[Must-match]` Overly long names have truncation/collision hazards, not a defined long-name namespace. `[May-improve]`: truncate every name to 31 characters before storing or comparing it. Import-call execution and its recovered argument-transfer defects remain covered by SPEC-004; export registration does not imply a repaired cross-program calling convention. `[Must-match]`

**Registry lifetime.** The two registries are emptied at different points. An entry is never removed one at a time except as listed. The table is `[Must-match]`.

| Event | Procedure registry | Variable registry |
| --- | --- | --- |
| A program executes `0x800E` | That program's entries are removed at once; the program is freed on the next VM time slice | Unchanged |
| A program executes `0x8009` | The old program's entries are removed after the replacement has started (see below) | Unchanged |
| A program ends through `0x8010`, stops through `0x8011`, or ends with a VM error | Unchanged: the program stays loaded until the next full clear | Unchanged |
| The program's object is destroyed or its script instance is removed during play | Unchanged: removing a script instance does not free its program | Unchanged |
| Leaving or loading a map (SPEC-006 §3.5 steps 1 and 2) | Empty: every loaded script program is ended and freed, which removes every entry | **Whole table cleared**, all owners |
| Full game reset: return to the main menu after a session, the start of every savegame restore, a failed savegame load (SPEC-014 §3.5, SPEC-008 §3.3) | Empty, as above | **Whole table cleared**, all owners |
| New game: map initialization resets to an empty map before the start map loads | Empty, as above | **Whole table cleared**, all owners |

The variable registry has no per-program cleanup. An exported variable's entry and value survive the removal of the program that exported it, but only until the next full clear. Values are never written to `SAVE.DAT` or to a `.SAV` map. Every map load therefore starts with an empty variable registry, and each exported name is re-created, with value I zero, by the first program that exports it on the new map. `[Must-match]`

Re-export works as follows:

- **After a map change** there is no conflict. 54 map scripts export the same names `Ian_ptr`, `Dog_ptr` and `Tycho_ptr` (`Katja_ptr` 53, `Tandi_ptr` 52). The next map's script finds the name absent, because the first map's entry was cleared in step 2 at the latest, and it becomes the new owner. Returning to the first map again creates a fresh entry. `[Must-match]`
- **On the same map**, a second export of a name by a different owner filename is a VM error. It ends the exporting program with the error state (SPEC-004 §3.8), so that script runs no further requests on this map. A second export by the same filename resets the value to I zero. `[Must-match]`
- **Corpus.** 57 corpus scripts export 196 distinct variable names, 528 exports in all, every one inside the program's initial stream (SPEC-004 §2.2). Fifty names are exported by more than one file. On every shipped map, with the map script and all object and spatial scripts counted, no name is exported by two different files, and no exporting file has two instances. No corpus initial stream or procedure body contains `0x8017`, `0x8008` through `0x800B`, `0x800E`, `0x8010` or `0x8011`; the only `0x8010` is the bootstrap's own at offset 18.
- **Imports.** An import is resolved when `0x8014` or `0x8015` executes, not when the program loads. On a new map, a script that reads a name before its exporter's first request has run gets the missing-variable VM error. The map script's first request (SPEC-006 §3.5 step 6) runs before the startup requests of every other script (step 8). `[Must-match]`

For `0x8009`, the replacement program is loaded and immediately runs one slice of its own initial stream. That slice has a 24-instruction budget, and instructions executed while critical is set do not count. Only then is the old program ended and its procedure entries removed. A replacement that exports a procedure name the old program still owns therefore raises a VM error. `[May-improve]` Safe rule: remove the old program's procedure entries before the replacement's first instruction. A replacement from a different file that exports a variable name owned by the old file is a VM error under either order, because variable entries outlive their program. `[Must-match]`

### 2.3 Complete General-Library Reference: `0x804C`-`0x80A0`

GUI effects refer to the current managed window or the calling program's selected window as specified. Coordinate scaling, optional-string checks, deferred callbacks, media waits, and incomplete dialogue backend behavior are expanded in section 3. Row tags follow the rule in section 1.

| Word | Mnemonic | Pop order | Push | Checks and observable host effect |
| --- | --- | --- | --- | --- |
| `0x804C` | `sayquit` | none | none | Request library dialogue exit/freeing; VM error if the host reports failure. `[Must-match]` |
| `0x804D` | `sayend` | none | none | Run library dialogue from saved start position under host-busy state. Recovered backend returns -1, which is ignored; no working dialogue loop is established. `[Must-match]` |
| `0x804E` | `saystart` | none | none | Reset start position to zero and request library dialogue creation; nonzero host result is a VM error. `[Must-match]` Backend has an initial-depth indexing defect; see 3.2. `[May-improve]`: index the new dialogue slot only after incrementing the depth. |
| `0x804F` | `saystartpos` | start position U | none | Store raw start position and request dialogue creation; no tag check. `[Must-match]` Same backend indexing defect as `saystart`. `[May-improve]`: same safe rule as `saystart`. |
| `0x8050` | `sayreplytitle` | title O | none | Replace the default library reply title; non-S clears it. Host failure would be a VM error. `[Must-match]` |
| `0x8051` | `saygotoreply` | reply name O | none | Select named reply case-insensitively; null text selects reply zero. Missing dialogue/reply is a VM error. `[Must-match]` |
| `0x8052` | `sayreply` | reply text O, label O | none | Request library reply definition; the inspected backend is an empty success stub, so it does not add a reply. `[Must-match]` |
| `0x8053` | `sayoption` | destination S or I, option text O | none | S destination names a reply; I names a procedure. Other destination tags are VM errors; host failure is a VM error. No current library reply means string-destination no-op or procedure-destination failure. `[Must-match]` |
| `0x8054` | `saymessage` | message O, title O | none | Request timed library message using shared timeout. Backend returns -1, so the opcode raises a VM error. `[Must-match]` |
| `0x8055` | `sayreplywindow` | background N, height U, width U, y U, x U | none | Store reply-window geometry and a copied/mangled background filename; only the background is type-checked. `[Must-match]` |
| `0x8056` | `sayoptionwindow` | background N, height U, width U, y U, x U | none | Store option-window geometry/background; same checks as reply-window configuration. `[Must-match]` |
| `0x8057` | `sayborder` | vertical border, horizontal border | none | Set library dialogue border sizes; no scaling or nonnegative clamp. `[Must-match]` |
| `0x8058` | `sayscrollup` | fourth image S/0/-1, third image U, second image U, first image U, y U, x U | none | Configure scroll-up control; first three image slots resolve only S and reject nonzero I. Fourth I -1 sets its special flag; fourth I zero means null. See 3.1 for the permissive other-tag checks. `[Must-match]` |
| `0x8059` | `sayscrolldown` | fourth image S/0/-1, third image U, second image U, first image U, y U, x U | none | Configure scroll-down control with the same checks and special fourth-image flag. `[Must-match]` |
| `0x805A` | `saysetspacing` | spacing | none | Set default library option spacing. `[Must-match]` |
| `0x805B` | `sayoptioncolor` | blue T, green T, red T | none | Store truncated RGB555 components from float channels times 31; no channel clamp. `[Must-match]` |
| `0x805C` | `sayreplycolor` | blue T, green T, red T | none | Store default reply RGB555 components, with the same T rule. `[Must-match]` |
| `0x805D` | `sayrestart` | none | none | Reset current library dialogue reply position to zero; absent dialogue is a VM error. `[Must-match]` |
| `0x805E` | `saygetlastpos` | none | I position | Return saved line plus saved reply number times 65536. `[Must-match]` |
| `0x805F` | `sayreplyflags` | flags | none | Store low 16 bits. `[Must-match]` Recovered behavior changes **option** flags, not reply flags. `[May-improve]`: store the value as the default reply flags and leave option flags unchanged. |
| `0x8060` | `sayoptionflags` | flags | none | Store low 16 bits of option flags. `[Must-match]` |
| `0x8061` | `saymessagetimeout` | timeout U | none | Store raw timeout payload. Reject only a masked tag exactly `0x4000`; ordinary I is not rejected. `[Must-match]` |
| `0x8062` | `createwin` | height U, width U, y U, x U, name U | none | Resolve name as text without checking its tag; scale geometry from 640x480 units and create window. Host creation failure is a VM error; does not push/select its ID. `[Must-match]` |
| `0x8063` | `deletewin` | name U | none | Resolve and delete named window; failure is a VM error; select the next window from the window-selection stack. `[Must-match]` |
| `0x8064` | `selectwin` | name S | none | Push/select named window, remember its ID for the program, and direct interpreter output there; missing window is a VM error. `[Must-match]` |
| `0x8065` | `resizewin` | height U, width U, y U, x U, name U | none | Resolve name and resize/reposition using scaled geometry; host failure is a VM error. `[Must-match]` |
| `0x8066` | `scalewin` | height U, width U, y U, x U, name U | none | Resolve name and rescale window contents/geometry; host failure is a VM error. `[Must-match]` |
| `0x8067` | `showwin` | none | none | Select the program's window and redraw it. `[Must-match]` |
| `0x8068` | `fillwin` | blue C, green C, red C | none | Select the program's window and fill it with the palette color derived from these channels. `[Must-match]` |
| `0x8069` | `fillrect` | blue C, green C, red C, height, width, y, x | none | Fill a rectangle in the program's window; unlike window creation, this wrapper passes geometry directly without 640x480 scaling. `[Must-match]` |
| `0x806A` | `fillwin3x3` | filename S | none | Load/mangle image, select program window, fill using nine-slice/3x3 treatment, then free image. Load failure is a VM error. `[Must-match]` |
| `0x806B` | `display` | filename S | none | Select program window, mangle filename, and display an image resource. No status result is pushed. `[Must-match]` |
| `0x806C` | `displaygfx` | height U, width U, y U, x U, filename U | none | Resolve/mangle filename without tag checks and display in a rectangle in the current managed window. `[Must-match]` |
| `0x806D` | `displayraw` | filename S | none | Select program window, mangle filename, display raw image bytes. `[Must-match]` |
| `0x806E` | `loadpalettetable` | filename U | none | S is normal; reject I except -1, but accepted non-S does not imply a default filename. Resolve text and replace palette/color-processing tables; load failure is a VM error. `[Must-match]` |
| `0x806F` | `fadein` | duration | none | Force black palette, then fade to current color palette over integer duration in host milliseconds while servicing background work; mark faded-in. `[Must-match]` |
| `0x8070` | `fadeout` | duration | none | Fade current palette to black, temporarily hiding/restoring cursor and servicing background work; mark faded-out. `[Must-match]` |
| `0x8071` | `gotoxy` | y, x | none | Select program window and move its text cursor; direct coordinates, not scaled geometry. `[Must-match]` |
| `0x8072` | `print` | value U | none | Select program window; S prints characters, F prints five fractional digits, I prints signed decimal. Other base tags produce no output. `[Must-match]` |
| `0x8073` | `format` | alignment, height, width, y, x, text S | none | Word-wrap/format text into a rectangle; not a conversion-template string-returning operation. Host formatting failure is a VM error. `[Must-match]` |
| `0x8074` | `printrect` | alignment, width, value U | none | Require alignment at most 2, without lower-bound check. Format S/F/I as for `print`, into a fixed 80-byte temporary text area, then print. `[Must-match]` Unsupported tags and long text are unsafe. `[May-improve]`: print nothing for an unsupported tag and truncate the formatted text to 79 bytes. |
| `0x8075` | `setfont` | font number | none | Change current font; wrapper raises a VM error only if host reports failure. `[Must-match]` |
| `0x8076` | `settextflags` | flags | none | Replace current text-rendering flags. `[Must-match]` |
| `0x8077` | `settextcolor` | blue T, green T, red T | none | Set current RGB555 text color; each float component is multiplied by 31 and truncated. `[Must-match]` |
| `0x8078` | `sethighlightcolor` | blue T, green T, red T | none | Set current RGB555 highlight color, with the same unchecked range and T rule. `[Must-match]` |
| `0x8079` | `stopmovie` | none | none | Select program window and request movie termination. `[Must-match]` |
| `0x807A` | `playmovie` | filename S | none | Append `.mve` if no dot occurs, select program window, install movie wait predicate, start playback; failure is a VM error. `[Must-match]` Waiting polarity is defective; see 3.2. `[May-improve]`: keep the program waiting until playback ends. |
| `0x807B` | `movieflags` | flags U | none | Apply raw movie-option bits without checking tag; no return value. `[Must-match]` |
| `0x807C` | `playmovierect` | height U, width U, y U, x U, filename I | none | Recovered check requires I for filename, then resolves it as text. Canonical I resolves to null and is copied as a filename: no safe ordinary typed call; S raises VM error. `[Must-match]` Using the resulting null text is a defect. `[May-improve]`: when the filename resolves to null, raise a VM error instead of copying it. |
| `0x807D` | Unregistered | none | none | Undefined-dispatch VM error; not an operand-consuming placeholder. `[Must-match]` |
| `0x807E` | Unregistered | none | none | Undefined-dispatch VM error. `[Must-match]` |
| `0x807F` | `addregion` | count, then repeated y and x, then name U | none | Variable-arity polygon registration; count includes the name. Coordinates are I and scaled; even count errors after point pops. `[Must-match]` The name test is permissive/defective; see 3.1. `[May-improve]`: reject a name operand that is not S with a VM error. |
| `0x8080` | `addregionflag` | flags, region name S | none | Set region flags; absent region/host failure is a VM error. `[Must-match]` |
| `0x8081` | `addregionproc` | release procedure, press procedure, exit procedure, enter procedure, region name S | none | Select program window and attach four callbacks to region; host failure is a VM error. `[Must-match]` |
| `0x8082` | `addregionrightproc` | release procedure, press procedure, region name S | none | Attach right-mouse callbacks; host failure is a VM error. `[Must-match]` |
| `0x8083` | `deleteregion` | name U | none | I other than -1 is a VM error; raw payload -1 deletes all regions. Otherwise resolve text; other non-S tags are not generally rejected. `[Must-match]` |
| `0x8084` | `activateregion` | event U, region name U | none | Resolve name without type checks and dispatch requested region event in the current managed window. `[Must-match]` |
| `0x8085` | `checkregion` | region name S | I bool | Return whether named region exists; does not test object visibility or point containment. `[Must-match]` |
| `0x8086` | `addbutton` | height, width, y, x, name S | none | Select program window and create scaled button; host return is ignored. `[Must-match]` |
| `0x8087` | `addbuttontext` | text S, button name S | none | Attach button text; host failure is a VM error. `[Must-match]` |
| `0x8088` | `addbuttonflag` | flags, button name S | none | Set button flags; host failure is a VM error. `[Must-match]` |
| `0x8089` | `addbuttongfx` | hover image U, normal image U, pressed image U, button name S | none | At least one image must be S or I zero, not necessarily all three. Resolve/mangle all images, select program window, assign graphics; host failure is a VM error. `[Must-match]` The one-valid-image admission is a defect. `[May-improve]`: treat each image operand that is not S as no image. |
| `0x808A` | `addbuttonproc` | release procedure, press procedure, exit procedure, enter procedure, button name S | none | Attach four button callbacks; host failure is a VM error. `[Must-match]` |
| `0x808B` | `addbuttonrightproc` | release procedure, press procedure, button name S | none | Recovered operation attaches **region** right-button callbacks using this name, not button callbacks. `[May-improve]`: attach the callbacks to the button of that name. |
| `0x808C` | `deletebutton` | name U | none | I -1 deletes all; other I is a VM error. Otherwise resolve name and delete button; failed deletion is a VM error. `[Must-match]` |
| `0x808D` | `hidemouse` | none | none | Hide cursor. `[Must-match]` |
| `0x808E` | `showmouse` | none | none | Show cursor. `[Must-match]` |
| `0x808F` | `mouseshape` | hotspot y, hotspot x, filename S | none | Load cursor image/shape and set hotspot; failure is a VM error. `[Must-match]` |
| `0x8090` | `refreshmouse` | fallback procedure | none | Refresh managed-window regions; execute procedure only if refresh reports no handled region activity. `[Must-match]` |
| `0x8091` | `setglobalmousefunc` | none | none | Immediately raises an unsupported-operation VM error; does not pop a callback operand. `[Must-match]` |
| `0x8092` | `addnamedevent` | procedure U, event name S | none | Register one-shot named event; no procedure-tag check, ignored host status. `[Must-match]` |
| `0x8093` | `addnamedhandler` | procedure U, event name S | none | Register persistent named handler; distinct from exported procedures; ignored host status. `[Must-match]` |
| `0x8094` | `clearnamed` | event name S | none | Remove named event/handler; missing-name status is ignored. `[Must-match]` |
| `0x8095` | `signalnamed` | event name S | none | Queue a named-event hit for later update; does not synchronously call it here. `[Must-match]` |
| `0x8096` | `addkey` | procedure, key number | none | Register key callback; -1 selects any-key callback; values above 255 error. `[Must-match]` Other negative indexes are not safely rejected. `[May-improve]`: raise the same VM error for key numbers below -1. |
| `0x8097` | `deletekey` | key number | none | Remove callback; -1 removes any-key handler. `[Must-match]` Same incomplete range validation. `[May-improve]`: same safe rule as `addkey`. |
| `0x8098` | `soundplay` | mode, filename S | I handle | Load/start sound in first free one of 32 slots; return `0xA0000000` combined with slot index, or -1 on failure. `[Must-match]` |
| `0x8099` | `soundpause` | handle | none | Pause ordinary stream or stop memory-backed sound; discard host success/failure. `[Must-match]` |
| `0x809A` | `soundresume` | handle | none | Unpause stream or restart memory-backed sound; discard status. `[Must-match]` |
| `0x809B` | `soundstop` | handle | none | Same operation as `soundpause`, not a distinct rewind/delete. `[Must-match]` |
| `0x809C` | `soundrewind` | handle | none | If playing, stop and restart; if not playing, leave it alone. No status is pushed. `[Must-match]` |
| `0x809D` | `sounddelete` | handle | none | Stop if necessary, delete sound, free its slot; no pushed status. `[Must-match]` |
| `0x809E` | `setoneoptpause` | enabled | none | Ensure shared dialogue media bit 8 is clear for nonzero input and set for zero input; name does not imply nonzero sets the bit. `[Must-match]` |
| `0x809F` | `selectfilelist` | filename pattern S, title S | D filename or I zero | Present matching-file selection; selected entry becomes D, cancel/no entries becomes I zero. Temporarily enters host-busy state. `[Must-match]` |
| `0x80A0` | `tokenize` | delimiter, previous S or I zero, source S | D token or I zero | First token uses leading source bytes. `[Must-match]` Continuation copies from the delimiter; see 3.1. `[Must-match]` A missing previous match and delimiter zero are unsafe. `[May-improve]`: return I zero when the previous text is absent, and never scan past the source terminator. |

### 2.4 Game Context and Variable State

The current script instance supplies owner/self, source, target, action, fixed parameter, result, and roll margin. They are not additional hidden stack operands. The three persistent variable families are separate from per-program stack globals and from exported variables. `[Must-match]`

| Word | Mnemonic | Pop order | Push | Checks and observable host effect |
| --- | --- | --- | --- | --- |
| `0x80A2` | `scr_return` | result | none | Set current script instance's result word; not a VM return or pushed result. Missing context produces no update. `[Must-match]` |
| `0x80B9` | `script_overrides` | none | none | Mark current script as overriding the default host action; missing context only diagnoses. No Boolean operand is consumed. `[Must-match]` |
| `0x80BC` | `self_obj` | none | I object | Current script owner, or null if none. `[Must-match]` |
| `0x80BD` | `source_obj` | none | I object | Current invocation's source, or zero with diagnostic when context is missing. `[Must-match]` |
| `0x80BE` | `target_obj` | none | I object | Current invocation's target, or zero with diagnostic. `[Must-match]` |
| `0x80BF` | `dude_obj` | none | I object | Current player reference. `[Must-match]` |
| `0x80C0` | `obj_being_used_with` | none | I object | Returns the same current target slot as `target_obj`; not a separately stored object. `[Must-match]` |
| `0x80C1` | `local_var` | element index | I value | Read current script's LVAR slice. Initial fallback -1; missing/system-script context leaves -1; out-of-pool reads normally return zero. Can lazily allocate locals. `[Must-match]` |
| `0x80C2` | `set_local_var` | value, element index | none | Set current script LVAR element; can allocate its slice. Host errors are not pushed. `[Must-match]` |
| `0x80C3` | `map_var` | element index | I value | Read map-wide MVAR; out of bounds gives zero with diagnostic. `[Must-match]` |
| `0x80C4` | `set_map_var` | value, element index | none | Set MVAR; out-of-bounds host failure is ignored, not returned. `[Must-match]` |
| `0x80C5` | `global_var` | element index | I value | Read campaign GVAR. Entire array absent gives -1; existing array with out-of-range index gives zero. `[Must-match]` |
| `0x80C6` | `set_global_var` | value, element index | none | Set campaign GVAR; absent array or out-of-range index only diagnoses/fails without a pushed status. `[Must-match]` |
| `0x80C7` | `script_action` | none | I action | Current script event/action code: the event number of the last request made to this instance, from the complete table in SPEC-004 §3.7 (12 critter cycle, 13 combat, 18 destroy, and so on); 0 before any request. Missing context gives zero. `[Must-match]` |
| `0x80EC` | `elevation` | object | I elevation | Object's elevation; null gives zero with diagnostic. `[Must-match]` |
| `0x80F7` | `fixed_param` | none | I value | Current invocation's fixed parameter; missing context gives zero. `[Must-match]` |
| `0x80FA` | `action_being_used` | none | I action | Current use-action selector; missing context gives -1. `[Must-match]` |
| `0x8101` | `cur_map_index` | none | I index | Current map-number association. `[Must-match]` |

### 2.5 Objects, Geometry Queries, Visibility, and Inventories

Unless a row explicitly checks category, a non-null I operand is assumed to reference a suitable live host object. Null diagnostics do not make arbitrary nonzero integers safe references. `[Must-match]`

| Word | Mnemonic | Pop order | Push | Checks and observable host effect |
| --- | --- | --- | --- | --- |
| `0x80A4` | `obj_name` | object | D name | Resolve object name. `[Must-match]` Null diagnoses but reuses the last name cached by this opcode, initially `<Critter>`, rather than returning I zero. `[May-improve]`: a null operand always returns D `<Critter>`. |
| `0x80A7` | `tile_contains_pid_obj` | PID, elevation, hex | I object | Find first placed object matching PID/hex/elevation; hex -1 or no match gives zero. `[Must-match]` |
| `0x80B6` | `move_to` | elevation, hex, object | I status | Move immediately, return 0 on success/-1 on failure, **not** resulting hex. Null gives -1. Player movement recenters and temporarily disables scroll restrictions; changing critter elevation removes combat membership. `[Must-match]` |
| `0x80B7` | `create_object_sid` | script-file number, elevation, hex, PID | I object | Create prototype instance and place it; hex -1 becomes 0. During save/load restoration return zero. Script number -1 keeps defaults; otherwise replace its script with index number minus 1. Partial failure can still return a created reference. `[Must-match]` If the prototype or object cannot be created and the script number is not -1, the reference touches a null object. `[May-improve]`: push I zero and skip script replacement when no object was created. Script number zero is covered in 3.4. |
| `0x80BA` | `obj_is_carrying_obj_pid` | PID, owner | I quantity | Count carried PID through inventory host traversal; null owner gives zero with diagnostic. `[Must-match]` |
| `0x80BB` | `tile_contains_obj_pid` | PID, elevation, hex | I bool | Test whether any placed object matches the three values; does not return its reference. `[Must-match]` |
| `0x80C8` | `obj_type` | object | I category | Extract category from object's art identifier, not necessarily its PID; null gives -1. `[Must-match]` |
| `0x80C9` | `obj_item_subtype` | object | I subtype | Require an item PID and available prototype to obtain subtype; otherwise -1. `[Must-match]` |
| `0x80D2` | `tile_distance` | second hex, first hex | I hops | Invoke reference hex-distance behavior; either input exactly -1 gives 9999. Boundary defects are retained, not corrected to Euclidean distance. `[Must-match]` |
| `0x80D3` | `tile_distance_objs` | second object, first object | I hops | Require non-null references whose unsigned payloads are at least 40000, same elevation and placed hexes; otherwise 9999. Low payloads are rejected as likely hex numbers. `[Must-match]` |
| `0x80D4` | `tile_num` | object | I hex | Read placed hex; null gives -1 with diagnostic. `[Must-match]` |
| `0x80D5` | `tile_num_in_direction` | steps, orientation, origin hex | I hex | Origin -1, orientation at least 6, or steps zero gives -1. Nonzero steps use parity/border behavior; a negative step count normally leaves origin unchanged. `[Must-match]` Negative orientation lacks a lower-bound check. `[May-improve]`: return -1 for a negative orientation. |
| `0x80D6` | `pickup_obj` | item | none | Null item no-op. Current invocation target acts as actor to pick up item. `[Must-match]` Recovered missing-script comparison checks the wrong failure value and can dereference missing context. `[May-improve]`: when no script matches the program, diagnose and return without action. |
| `0x80D7` | `drop_obj` | item | none | Current invocation target drops item; missing context, null target/item gives no action. `[Must-match]` |
| `0x80D8` | `add_obj_to_inven` | item, owner | none | Add one item only if both non-null and item has no owner; successful add disconnects it from map and refreshes display. Already-owned item diagnoses. `[Must-match]` |
| `0x80D9` | `rm_obj_from_inven` | item, owner | none | Remove one; on success connect it at hex 1/elevation 0, refresh, repair equipment art/armor/UI as needed (worn-armor art: section 3.8). Null operands no-op. `[Must-match]` |
| `0x80DA` | `wield_obj_critter` | item, critter | none | Require non-null operands and critter PID. Equip right hand, except player's active hand selects left/right; wearing player armor adjusts armor class. Armor art rules: section 3.8. `[Must-match]` |
| `0x80DB` | `use_obj` | object | none | Require object, current script, and invocation target. Critter owner queues target actor's use; non-critter owner directly uses object. Missing actor/context only diagnoses. `[Must-match]` |
| `0x80DC` | `obj_can_see_obj` | target, observer | I bool | Require both references and placed target; check perception and straight-path blocker equal to target. Null returns zero with diagnostic. `[Must-match]` |
| `0x80E1` | `turn_off_objs_in_area` | last hex, first hex, last elevation, first elevation | none | Hide matching visible objects. `[Must-match]` Recovered range sorting and elevation iteration are defective and can loop forever; see 3.4. `[May-improve]`: swap reversed hex bounds and advance one elevation per pass so the call terminates. |
| `0x80E2` | `turn_on_objs_in_area` | last hex, first hex, last elevation, first elevation | none | Unhide matching hidden objects. `[Must-match]` Same nonterminating elevation-loop and hex-bound defects. `[May-improve]`: same safe rule as `turn_off_objs_in_area`. |
| `0x80E3` | `set_obj_visibility` | invisible flag, object | none | Nonzero means **hide**, zero means show. For critters also set/clear movement-nonblocking. Null or save/load restoration suppresses change. `[Must-match]` |
| `0x80F4` | `destroy_object` | object | none | Remove inventory stack/map object, clear animation/combat membership and refresh. Null no-op; critter destruction refused during restoration. Self destruction cancels further program execution; details in 3.4. `[Must-match]` |
| `0x80F5` | `obj_can_hear_obj` | second object, first object | I bool on valid non-null path | Recovered condition is inverted: two non-null objects return zero without hearing test; a null operand enters a dereferencing path and is unsafe. `[May-improve]`: push 1 only when both objects are non-null, on the same elevation, both placed, and the perception test passes; otherwise push 0. |
| `0x80FF` | `critter_attempt_placement` | elevation, desired hex, object | I status, or no push on null | On elevation change remove critter from combat, first move object to hex 0, then try placement near requested hex. `[Must-match]` Null diagnoses and returns **without** a result. `[May-improve]`: on null, diagnose and push I -1. |
| `0x8100` | `obj_pid` | object | I PID | Return prototype identifier; null gives -1 with diagnostic. `[Must-match]` |
| `0x8104` | `proto_data` | member number, PID | I or D | Read selected prototype member; unavailable/unimplemented member normally I zero. Names/descriptions normally D. `[Must-match]` The miscellaneous-description path incorrectly returns I host text-reference bits; see 3.6. `[May-improve]`: return that description as D like the other categories. |
| `0x8106` | `critter_inven_obj` | selector, critter | I object/count | Select 0 worn, 1 right hand, 2 left hand, -2 direct entry count. Player hand queries only return active hand. Non-critter gives zero. `[Must-match]` Null is unchecked and unsafe. `[May-improve]`: null gives I zero with a diagnostic. |
| `0x8107` | `obj_set_light_level` | radius, intensity percent, object | none | Null no-op. Request intensity zero, or truncated `percent * 65636 / 100` for nonzero percent: coefficient is **65636**, not 65536. Set light and refresh on success. `[Must-match]` |
| `0x810D` | `obj_carrying_pid_obj` | PID, owner | I object | Return carried representative object matching PID; null/no match gives zero. `[Must-match]` |
| `0x8116` | `add_mult_objs_to_inven` | quantity, item, owner | none | Force-add requested quantity, then disconnect map representative on success. Null operands no-op; unlike single-add wrapper, no already-owned check here. `[Must-match]` |
| `0x8117` | `rm_mult_objs_from_inven` | requested quantity, item, owner | I quantity, or no push on null | Compute minimum of carried/requested count; attempt removal and connection at hex 1/elevation 0, update equipment UI. Push computed count even if removal fails. `[Must-match]` Null returns with no result. `[May-improve]`: on null, push I zero. |
| `0x8124` | `party_add` | object | none | Add non-null object to party; null diagnoses. `[Must-match]` |
| `0x8125` | `party_remove` | object | none | Remove non-null party member; null diagnoses. `[Must-match]` |
| `0x812C` | `inven_unwield` | none | none | Unwield current script owner's right hand, or active player hand. No object operand is consumed; missing owner is not checked by the wrapper. `[Must-match]` |
| `0x812D` | `obj_is_locked` | object | I bool | Read container/door lock state; null gives zero with diagnostic. `[Must-match]` |
| `0x812E` | `obj_lock` | object | none | Lock supported non-null container/door; discard host status. `[Must-match]` |
| `0x812F` | `obj_unlock` | object | none | Unlock supported non-null container/door; discard status. `[Must-match]` |
| `0x8130` | `obj_is_open` | object | I bool | Non-null object's frame number is nonzero; does not independently require an openable subtype. Null gives zero. `[Must-match]` |
| `0x8131` | `obj_open` | object | none | Ask host to open supported object if currently closed; null diagnoses. Lock/openability failures are not pushed. `[Must-match]` |
| `0x8132` | `obj_close` | object | none | Ask host to close supported object if currently open; null diagnoses. `[Must-match]` |
| `0x8138` | `item_caps_total` | owner | I amount | Sum money stacks, including contained containers; null gives zero. `[Must-match]` |
| `0x8139` | `item_caps_adjust` | amount, owner | I status | Add/remove currency using inventory traversal; normal success 0, insufficient negative balance or host failure -1. Null gives -1. `[Must-match]` |
| `0x8144` | `destroy_mult_objs` | requested quantity, object | I count | Owned item: destroy up to requested/carried stack count and return it. Unowned object is erased but returns zero. Self destruction cancels execution. `[Must-match]` Null is unchecked and unsafe. `[May-improve]`: null pushes I zero with no effect. |
| `0x8145` | `use_obj_on_obj` | target, item | none | Require item, target, current script and suitable self. Critter self queues use-item-on-target; other self performs direct use. `[Must-match]` |
| `0x8147` | `move_obj_inven_to_obj` | destination owner, source owner | none | Transfer all source inventory to destination; repair removed weapon/armor art and player equipment UI. Null operands diagnose/no-op. `[Must-match]` |
| `0x8149` | `obj_art_fid` | object | I art ID | Return art identifier; null gives zero with diagnostic. `[Must-match]` |
| `0x814A` | `art_anim` | art identifier | I animation | Extract bits 16-23; no object is dereferenced. `[Must-match]` |
| `0x814B` | `party_member_obj` | PID | I object | Locate party representative by PID, or zero. `[Must-match]` |
| `0x814C` | `rotation_to_tile` | target hex, source hex | I orientation | Quantize the relative projected screen angle into six directions; vertical difference chooses northeast when above, southeast otherwise. `[Must-match]` Invalid hexes are not safely checked. `[May-improve]`: push I 0 with a diagnostic when either hex is outside 0-39999. |
| `0x814D` | `jam_lock` | object | none | Ask host to jam supported lock; discard status. `[Must-match]` |
| `0x8150` | `obj_on_screen` | object | I bool | Same elevation and object bounds intersect inclusive rectangle `(0,0)`-`(640,480)`; not a pixel-visibility/occlusion test. Null gives zero. `[Must-match]` |

### 2.6 Animation Registration and Immediate Animation

Most registration wrappers consume and check their operands even when combat prevents the effect. They do not push the animation scheduler's status. `[Must-match]`

| Word | Mnemonic | Pop order | Push | Checks and observable host effect |
| --- | --- | --- | --- | --- |
| `0x80CC` | `animate_stand_obj` | object | none | Null substitutes self if script context exists. Outside combat, begin unreserved sequence, register standing, end sequence. `[Must-match]` |
| `0x80CD` | `animate_stand_reverse_obj` | object | none | Same fallback/gate; register standing in reverse. `[Must-match]` |
| `0x80CE` | `animate_move_obj_to_tile` | movement flags, hex, object | none | Require non-null active object, valid script context, hex greater than -1, and no combat. Bit `0x10` clears queued animation; remove that bit, then zero means walk and any nonzero means run. Begin/end one unreserved sequence. `[Must-match]` |
| `0x80CF` | `animate_jump` | none | none | Diagnostic-only unsupported action; pops no purported jump arguments and pushes nothing. `[Must-match]` |
| `0x80E7` | `anim_busy` | object | I state | Matching active nontrivial animation gives -1, not 1; no match/null gives zero. Callbacks, the currently constructed sequence, and a lone standing action do not count; a full construction list also forces zero. `[Must-match]` |
| `0x810C` | `anim` | frame/direction control, animation selector, object | none | Non-null required. Selectors below 65 register forward for control zero, reverse otherwise, with death-art/status adjustments. Selector 1000 sets orientation if control is below 6; 1010 sets frame. Lower bounds are incompletely checked. `[Must-match]` |
| `0x810E` | `reg_anim_func` | parameter, command | none | Outside combat: command 1 begins sequence with parameter flags; 2 clears animation of object parameter; 3 ends sequence and ignores parameter. Unknown command no-op; still two pops. `[Must-match]` |
| `0x810F` | `reg_anim_animate` | delay, animation, object | none | Outside combat register forward animation for non-null object. Special PID `0x0100002F`/animation 20 is suppressed unless violence preference exists and is at least 2. `[Must-match]` |
| `0x8110` | `reg_anim_animate_reverse` | delay, animation, object | none | Outside combat register reverse animation; null diagnoses. `[Must-match]` |
| `0x8111` | `reg_anim_obj_move_to_obj` | delay, destination object, moving object | none | Outside combat register walk toward destination, unlimited action points. Only moving object is null-checked here. `[Must-match]` |
| `0x8112` | `reg_anim_obj_run_to_obj` | delay, destination object, moving object | none | Same, running rather than walking. `[Must-match]` |
| `0x8113` | `reg_anim_obj_move_to_tile` | delay, hex, object | none | Outside combat register walk on object's current elevation, unlimited action points; null diagnoses. `[Must-match]` |
| `0x8114` | `reg_anim_obj_run_to_tile` | delay, hex, object | none | Same, running. `[Must-match]` |
| `0x8126` | `reg_anim_animate_forever` | animation, object | none | Outside combat register repeating animation with delay -1; only two operands. `[Must-match]` |
| `0x813A` | `anim_action_frame` | animation, object | I frame | Build art selection with weapon animation zero and raw object orientation, load art and read its action frame. Missing object/art gives zero. No animation is scheduled. `[Must-match]` |
| `0x813B` | `reg_anim_play_sfx` | delay, sound name S, object | none | Register object sound cue; null object diagnoses. Unlike adjacent registration wrappers, it does **not** apply a no-combat gate. `[Must-match]` |

### 2.7 Game Dialogue and Barter

These instructions use the game's GDialog subsystem, not the incomplete general-library `say` subsystem. Text operands accepted as S are literal text; I text operands select a message number in the specified message list. `[Must-match]`

| Word | Mnemonic | Pop order | Push | Checks and observable host effect |
| --- | --- | --- | --- | --- |
| `0x80DE` | `start_gdialog` | background, head art number, reaction, object, unused list number | none | Consume five I values. No-op in combat or with null object. Configure head/background, mood and script context; actual dialogue target becomes self, not necessarily the passed object. Head-enabled path first consults the previous dialogue target; see 3.3. `[Must-match]` A missing previous target is unsafe. `[May-improve]`: if no dialogue target exists yet, derive the mood from the new self target. |
| `0x80DF` | `end_dialogue` | none | none | Ask game dialogue to exit; clear dialogue target/script association only if exit does not return -1. `[Must-match]` |
| `0x80E0` | `dialogue_reaction` | reaction | none | Set shared mood and request talking-head reaction: usual values -1 good, 0 neutral, 1 bad. `[Must-match]` |
| `0x80E5` | `barter_offer` | third value, second value, first value | none | All I checked, then ignored. Registered empty barter stub. `[Must-match]` |
| `0x80E6` | `barter_asking` | third value, second value, first value | none | All I checked, then ignored. No computed price or pushed result. `[Must-match]` |
| `0x80F9` | `dialogue_system_enter` | none | none | Require script context, active critter self if applicable, and no combat; request dialogue game state and set target to self. Does not consume a target operand. `[Must-match]` |
| `0x811C` | `gsay_start` | none | none | Start game dialogue under host-busy flag; nonzero start status raises VM error. `[Must-match]` |
| `0x811D` | `gsay_end` | none | none | Run game dialogue interaction under host-busy flag; no numeric result is pushed. `[Must-match]` |
| `0x811E` | `gsay_reply` | message I or S, message-list number | none | Set reply from message catalog or literal string; both forms retain the I list operand. `[Must-match]` |
| `0x811F` | `gsay_option` | reaction, procedure number, message I or S, message-list number | none | Add procedure-backed option. Procedure **must be I**: an apparent later string-procedure branch is unreachable because earlier validation rejects it. `[Must-match]` |
| `0x8120` | `gsay_message` | reaction, message I or S, message-list number | none | Set reply and show message, adding a fixed continuation option. Supplied reaction is checked but ignored; continuation reaction is always 50. `[Must-match]` |
| `0x8121` | `giq_option` | reaction, procedure number, message I or S, message-list number, IQ threshold | none | Same option type checks as `gsay_option`; add only if player intelligence plus Smooth Talker rank meets threshold. Negative threshold means intelligence at most its absolute value. `[Must-match]` |
| `0x8129` | `gdialog_barter` | activation parameter | none | Activate game barter; failure only diagnoses. `[Must-match]` |
| `0x814E` | `gdialog_set_barter_mod` | modifier | none | Replace game-dialogue barter modifier. `[Must-match]` |

### 2.8 Critter Stats, Skills, Rolls, Traits, and Health

Object/skill/stat operands are still integer-tagged. Host routines can enforce additional stat/skill limits; the opcode's I test alone does not validate those domains. `[Must-match]`

| Word | Mnemonic | Pop order | Push | Checks and observable host effect |
| --- | --- | --- | --- | --- |
| `0x80A1` | `give_exp_points` | experience | none | Add player XP with experience perks/clamps and level-up effects; failure diagnoses but pushes no status. `[Must-match]` |
| `0x80A6` | `get_pc_stat` | player-stat number | I value | Read player-only statistic; unsupported index gives zero. `[Must-match]` |
| `0x80AA` | `has_skill` | skill number, object | I level | Return skill level for critter object; null/non-critter gives zero. This is not just a Boolean skill-presence test. `[Must-match]` |
| `0x80AB` | `using_skill` | skill number, object | I bool | Only reports player sneaking when skill is sneak; all other combinations give zero. `[Must-match]` |
| `0x80AC` | `roll_vs_skill` | modifier, skill number, object | I roll | Critter plus current script required; roll skill, store margin for `how_much`. Fallback roll is critical failure 0. `[Must-match]` |
| `0x80AD` | `skill_contest` | third value, second value, first value | I zero | All I checked; unimplemented contest diagnoses and returns zero. `[Must-match]` |
| `0x80AE` | `do_check` | modifier, primary-stat number, object | I roll | With object/context, roll one of primary stats 0-6 and save margin; unsupported stat or unavailable context yields zero. `[Must-match]` |
| `0x80AF` | `is_success` | roll | I predicate | Roll 0/1 gives 0, 2/3 gives 1, other values give -1. `[Must-match]` |
| `0x80B0` | `is_critical` | roll | I predicate | Roll 0/3 gives 1, 1/2 gives 0, other values give -1. `[Must-match]` |
| `0x80B1` | `how_much` | ignored value | I margin | Consume and check one I even though ignored; return current script's last stored roll margin, or zero without context. `[Must-match]` |
| `0x80B2` | `reaction_roll` | third value, second value, first value | I zero | Three checked I inputs, but recovered reaction calculation is a zero-returning stub. `[Must-match]` |
| `0x80B3` | `reaction_influence` | third value, second value, first value | I zero | Three checked I inputs, then zero-returning stub with no influence update. `[Must-match]` |
| `0x80B4` | `random` | maximum, minimum | I value | Normal mode samples inclusive host range. Recording/playback mode instead returns truncated `(maximum - minimum) / 2`, without adding minimum. `[Must-match]` |
| `0x80B5` | `roll_dice` | second value, first value | I zero | Checked two-operand unimplemented dice roll; diagnoses and returns zero. `[Must-match]` |
| `0x80CA` | `get_critter_stat` | stat number, object | I value | Read effective stat; null gives -1 with diagnostic. Wrapper does not check critter category. `[Must-match]` |
| `0x80CB` | `set_critter_stat` | adjustment, stat number, object | I status | Only player may change. Add adjustment to base stat, rather than assigning it; return 0 even if underlying setter rejects, -1 for null/non-player. `[Must-match]` |
| `0x80E8` | `critter_heal` | hit-point adjustment, object | I zero on normal path | Adjust hits, cap at maximum, kill if reduced to nonpositive, and refresh player HP display. `[Must-match]` Null is not checked and is unsafe. `[May-improve]`: null pushes I zero with no effect. |
| `0x80F3` | `has_trait` | selector, object, kind | I value | Kind 0 queries player perk, kind 2 player trait, kind 1 object attribute. Requires non-null object even for player-global queries; unsupported cases give zero. Selectors in 3.5. `[Must-match]` |
| `0x80FB` | `critter_state` | object | I flags | Null/non-critter/inactive gives 1. Active critter gives 0, or prone bit 2 for art animations 48-49, combined with its cripple flags. `[Must-match]` |
| `0x80FD` | `radiation_inc` | amount, object | none | Null no-op. Only player is adjusted by host; positive exposure is resistance-reduced and may produce radiation/geiger state/messages. `[Must-match]` |
| `0x80FE` | `radiation_dec` | amount, object | none | Null no-op. If current radiation is nonnegative, request negative amount; host adjusts only player and clamps at zero. Negative supplied amount can increase exposure. `[Must-match]` |
| `0x8102` | `critter_add_trait` | value, selector, kind, object | I -1 | For critters, kind 0 adds a **player** perk; kind 1 selector 5 sets AI, 6 changes team except during restoration. Always pushes -1, even on success or null. `[Must-match]` |
| `0x8103` | `critter_rm_trait` | ignored value, selector, kind, object | I -1, or no push on null | Perk-removal branch is empty; unsupported kind diagnoses. `[Must-match]` Null returns before result push. `[May-improve]`: on null, push I -1. |
| `0x8122` | `poison` | adjustment, object | none | Null diagnoses. Only player poison is adjusted; positive input is resistance-reduced, poison clamps at zero, and poison queue/messages may change. NPC request diagnoses failed adjustment. `[Must-match]` |
| `0x8123` | `get_poison` | object | I amount | Read poison for critter; null/non-critter gives zero. `[Must-match]` |
| `0x8127` | `critter_injure` | injury mask, object | none | Null no-op; combine mask with cripple bits `0x7C` only, then set those bits. Does not heal existing injuries; refresh player equipment for arm injuries. `[Must-match]` |
| `0x812A` | `difficulty_level` | none | I difficulty | Read game difficulty preference; default normal, numeric 1. `[Must-match]` |
| `0x812B` | `running_burning_guy` | none | I preference | Return running-burning-animation preference; default 1. `[Must-match]` |
| `0x813C` | `critter_mod_skill` | points, skill number, object | I zero | Only player critter changes. Add points to unspent skill pool, then perform one skill-point increment per positive point if pool update succeeds. Negative points reduce pool without decrementing skill. Always return zero. `[Must-match]` |

### 2.9 Combat, Time, Lighting, and Map Transitions

Map/world/combat requests are often deferred state requests, not synchronous changes to the script instruction address. Clock units are ten game ticks per second. `[Must-match]`

| Word | Mnemonic | Pop order | Push | Checks and observable host effect |
| --- | --- | --- | --- | --- |
| `0x80A8` | `set_map_start` | orientation, elevation, y, x | none | Compute hex `200 * y + x`, set current elevation/camera and future entrance defaults. Failed elevation/camera selection diagnoses and returns. Does not itself move player. `[Must-match]` |
| `0x80A9` | `override_map_start` | orientation, elevation, y, x | none | Move player/set orientation and recenter immediately; does not replace entrance-default header fields. Failed move retries previous camera hex on requested elevation; failed recovery terminates process, not just VM. `[Must-match]` |
| `0x80D0` | `attack` | eighth value, seventh value, maximum damage, minimum damage, accuracy, ignored third value, ignored second value, target | none | Eight I pops; same registration as `0x80DD`. Use current self as attacker, require active/nonfleeing target; request combat or set existing-combat opponent. Extra pair controls special request mode when equal. See 3.5. `[Must-match]` The active-dialogue early return leaks host-busy state. `[May-improve]`: clear the host-busy bit on every return path. |
| `0x80D1` | `make_daytime` | none | none | Registered empty operation; does not change the clock or lighting. `[Must-match]` |
| `0x80DD` | `attack` alias | eighth value, seventh value, maximum damage, minimum damage, accuracy, ignored third value, ignored second value, target | none | Exact alias of `0x80D0`, not a two-object attack instruction. `[Must-match]` Same host-busy leak as `0x80D0`. `[May-improve]`: same safe rule as `0x80D0`. |
| `0x80E4` | `load_map` | entrance parameter, map I or S | none | Numeric nonnegative index or matched map name; store load-map campaign parameter and request transition with entrance hex/elevation/orientation all -1. Unmatched/negative map produces no transition. `[Must-match]` |
| `0x80E9` | `set_light_level` | level | none | Request piecewise ambient intensity with special level-50 case; exact non-linear arithmetic in 3.5, not simply percent times 65536. `[Must-match]` |
| `0x80EA` | `game_time` | none | I ticks | Current game timestamp. `[Must-match]` |
| `0x80EB` | `game_time_in_seconds` | none | I seconds | Current game ticks divided by 10, fractional part discarded. `[Must-match]` |
| `0x80ED` | `kill_critter` | death animation, object | none | Null no-op. Clear animation/combat membership and kill; self sets execution-cancellation bit. During restoration this opcode warns but **continues**, unlike mass kill. `[Must-match]` |
| `0x80EE` | `kill_critter_type` | death mode, PID | none | Affect matching visible/live non-final-death-art objects. Mode 0 erases, 1 cycles preset death art, other nonzero uses static fall-back death. Refused during restoration; loop has a count guard and updates last-visit time. `[Must-match]` |
| `0x80EF` | `critter_damage` | damage type/flags, amount, object | none | Require critter; apply fixed amount as both damage bounds at its hex. Bit `0x100` bypasses armor, `0x200` suppresses animation. `[Must-match]` Null/non-critter early returns leak host-busy state. `[May-improve]`: clear the host-busy bit on every return path. |
| `0x80F0` | `add_timer_event` | fixed parameter, delay ticks, object | none | Queue script timer for non-null object's script reference. Delay is not converted from seconds here. `[Must-match]` |
| `0x80F1` | `rm_timer_event` | object | none | Remove **all queued events for this object**, not only script timer events. Null diagnoses/no-op. `[Must-match]` |
| `0x80F2` | `game_ticks` | seconds | I ticks | Clamp negative input to zero, then multiply by 10. `[Must-match]` Oversized arithmetic is not safely range-validated. `[May-improve]`: wrap the product modulo 2^32 as two's complement. |
| `0x80F6` | `game_time_hour` | none | I clock | Current time in HHMM form, not raw elapsed hours. `[Must-match]` |
| `0x80F8` | `tile_is_visible` | hex | I bool | Rough numeric-strip heuristic: true when absolute camera-hex difference modulo 200 is below 5 **or** its quotient by 200 is below 5. No actual screen/occlusion test. `[Must-match]` |
| `0x80FC` | `game_time_advance` | ticks | none | Advance one full 864000-tick day at a time and process queued events after each, then advance remainder and process again. Can invoke queued host/script effects. `[Must-match]` |
| `0x8108` | `world_map` | none | none | Queue world-map request; it opens the current town's town map first, then the world map (SPEC-014 §3.39). `[Must-match]` |
| `0x8109` | `town_map` | none | none | Queue town-map request; it opens the world map directly, with no town map (SPEC-014 §3.39). `[Must-match]` |
| `0x810B` | `metarule` | parameter, rule | I value | Rules 13 end-game request, 14 first-run test, 15 elevator request, 16 party count, 22 loading-state test; other rules return zero. `[Must-match]` |
| `0x8118` | `get_month` | none | I month | Current calendar month, numbered 1-12. `[Must-match]` |
| `0x8119` | `get_day` | none | I day | Current calendar day of month, numbered from 1. `[Must-match]` |
| `0x811A` | `explosion` | maximum damage, elevation, hex | none | Hex -1 diagnoses/no-op. Post a deferred explosion request with minimum damage 1, except maximum zero gives minimum zero; servicing, damage and the no-retry rule are in SPEC-013 §3.10. `[Must-match]` |
| `0x811B` | `days_since_visited` | none | I days | Truncated elapsed ticks divided by 864000; last-visit timestamp zero gives -1. `[Must-match]` |
| `0x8128` | `combat_is_initialized` | none | I bool | Current combat-active state. `[Must-match]` |
| `0x8143` | `attack_setup` | defender, attacker | none | Two-object combat request with accuracy bonus 0 and damage range 0-2147483647; active attacker/defender required. Existing combat sets opponent/maneuver rather than starting a second combat. `[Must-match]` Null defender is not reliably checked. `[May-improve]`: a null defender is a no-op with a diagnostic. |
| `0x8146` | `endgame_slideshow` | none | none | Queue endgame slideshow under host-busy state. `[Must-match]` |
| `0x8148` | `endgame_movie` | none | none | Play host endgame movie sequence under host-busy state. `[Must-match]` |
| `0x814F` | `combat_difficulty` | none | I preference | Read combat difficulty preference; default 0. `[Must-match]` |
| `0x8151` | `critter_is_fleeing` | object | I bool | Read combat maneuver bit `0x04`; null gives zero with diagnostic. No category check. `[Must-match]` |
| `0x8152` | `critter_set_flee_state` | enabled, object | none | Set or clear maneuver bit `0x04`; null diagnoses. `[Must-match]` |
| `0x8153` | `terminate_combat` | none | none | During combat, set deferred quit-combat request; otherwise no-op. Does not terminate caller immediately. `[Must-match]` |
| `0x8155` | `critter_stop_attacking` | object | none | Non-null: set maneuver bit `0x02` and clear last-opponent reference; null diagnoses. `[Must-match]` |

### 2.10 Game Text, Audio, Movies, and UI

| Word | Mnemonic | Pop order | Push | Checks and observable host effect |
| --- | --- | --- | --- | --- |
| `0x80A3` | `play_sfx` | sound name S | none | Play one game sound effect; no library sound handle is returned. `[Must-match]` |
| `0x80A5` | `sfx_build_open_name` | action, object | D name | Build scenery-door or item-container sound name from prototype sound byte. Null diagnoses and pushes D offset zero, not I null. `[Must-match]` |
| `0x80B8` | `display_msg` | text S | none | Append game message-log text; optionally also log script messages to debug output. `[Must-match]` Text is passed as a debug format when debug display is enabled. `[May-improve]`: emit the debug text literally. |
| `0x8105` | `message_str` | message number, message-list number | D text | Positive message number fetches catalog text via speech-aware lookup; missing/nonpositive message returns D `Error`. Can have speech-host effects. `[Must-match]` |
| `0x810A` | `float_msg` | style, text U, object | none | Only S text is resolved; other text tags are not rejected but become null and produce no message. Require object on current elevation and nonempty text; create floating text with style/font/color rules in 3.5, always with a one-pixel outline in key 0 [228]. Layout, placement, lifetime, the 19-text limit, replacement of the owner's earlier text and removal are in SPEC-014 §3.46. A refused creation is silent. `[Must-match]` |
| `0x8115` | `play_gmovie` | movie number | none | Suspend dialogue background updates, play indexed game movie with per-movie fade/music options, restore updates. `[Must-match]` Index is not checked before indexing its options table. `[May-improve]`: ignore numbers outside 0-13 with a diagnostic. |
| `0x8133` | `game_ui_disable` | none | none | Disable game UI/input/cursor controls. `[Must-match]` |
| `0x8134` | `game_ui_enable` | none | none | Reenable game UI/input/cursor controls. `[Must-match]` |
| `0x8135` | `game_ui_is_disabled` | none | I bool | Return UI disabled state. `[Must-match]` |
| `0x8136` | `gfade_out` | enabled | none | Nonzero fades to black using game palette timing; zero only diagnoses. Operand is a gate, not duration. `[Must-match]` |
| `0x8137` | `gfade_in` | enabled | none | Nonzero fades to game color palette; zero only diagnoses. `[Must-match]` |
| `0x813D` | `sfx_build_char_name` | extra selector, animation, object | D name | Construct uppercase character effect name using art and sound selectors. Null pushes D offset zero. `[Must-match]` Unavailable art/name can cause unchecked text-copy failure. `[May-improve]`: push empty D text when the art or name is unavailable. |
| `0x813E` | `sfx_build_ambient_name` | supposed base name I | D only if downstream operations succeed | Wrong tag requirement: S is rejected, canonical I then resolves to null text. No defined ordinary string-input result; not an integer-to-string conversion. `[Must-match]` Using the resulting null text is a defect. `[May-improve]`: keep the I tag check and raise a VM error when the base name resolves to null. |
| `0x813F` | `sfx_build_interface_name` | supposed base name I | D only if downstream operations succeed | Same wrong I-versus-string requirement, then interface-name builder. `[Must-match]` Using the resulting null text is a defect. `[May-improve]`: same safe rule as `sfx_build_ambient_name`. |
| `0x8140` | `sfx_build_item_name` | supposed base name I | D only if downstream operations succeed | Same wrong type requirement; additionally calls **interface** naming, not a distinct item-name builder. `[Must-match]` Using the resulting null text is a defect. `[May-improve]`: same safe rule as `sfx_build_ambient_name`. |
| `0x8141` | `sfx_build_weapon_name` | target, hit mode, weapon, sound-effect type | D name | Build uppercase weapon sound name from effect/weapon/mode/material. `[Must-match]` Enum and reference bounds are not checked by this wrapper. `[May-improve]`: raise a VM error for an out-of-range sound-effect type or hit mode. |
| `0x8142` | `sfx_build_scenery_name` | active/passive selector, action, supposed base name I | D only if downstream operations succeed | All three I checked; base is then resolved as text and canonical I produces null. String input is rejected. `[Must-match]` Using the resulting null text is a defect. `[May-improve]`: same safe rule as `sfx_build_ambient_name`. |
| `0x8154` | `debug_msg` | text S | none | Emit debug text only when script-message debug preference is enabled. `[Must-match]` Unchecked format interpretation is retained. `[May-improve]`: emit the debug text literally. |

## 3. Behavioral Rules & Edge Cases

### 3.1 String Resolution, Formatting, and Managed GUI Operands

String resolution first tests raw tag bit `0x0800` to select the dynamic heap, otherwise bit `0x1000` to select the static literal area, otherwise returns null text. This is separate from the masked I/S/F validation. A canonical I payload is therefore **not** automatically treated as a static-string offset. This explains the broken normal paths of `playmovierect` and four game sound-name wrappers: they require I, then use string resolution anyway. Their type check must not be silently replaced with S in a compatibility specification. `[Must-match]` The later use of the resulting null text is a defect; the affected rows state their safe rules `[May-improve]`.

Optional-text O and optional-filename N are different. O maps every non-S tag to null; N accepts only S or integer zero and explicitly maps zero to null. Other permissive checks are not a general null-filename convention. In particular, allowing integer -1 through palette loading does not establish a working default palette path. `[Must-match]` Using the resulting null text is a defect. `[May-improve]`: when the palette filename resolves to null, raise a VM error instead of loading.

Window creation/resizing/scaling, button creation, and region-point registration apply the integer transformations `trunc((coordinate * horizontal resolution + 639) / 640)` and `trunc((coordinate * vertical resolution + 479) / 480)`. The horizontal rule also applies to widths, the vertical rule to heights. Truncation is toward zero, including for negative numerators; there is no universal coordinate clamp. Other wrappers pass geometry directly, so scaling must not be added globally. `[Must-match]`

Color channels are interpreted as binary32 payloads after the C/T rules, then multiplied by 31 and truncated for RGB555 lookup/state. There is no general 0-1 range check. `[Must-match]` Nonfinite or oversized values have no safely defined conversion. `[May-improve]`: treat a nonfinite channel as 0 and clamp the truncated channel to 0-31. Integer 1 is specially converted for C fills but is rejected by T text/dialogue colors. Numeric-looking strings are not parsed as numbers. `[Must-match]`

`print` converts only S/F/I and uses five fractional digits for F. `format` positions/wraps existing text and pushes no formatted string. `[Must-match]` `printrect` has a fixed 80-byte text buffer with unchecked copying/format expansion. `[May-improve]`: truncate the formatted text to 79 bytes. Game `display_msg` and `debug_msg` pass enabled debug text as a native format rather than as a guaranteed literal; embedded formatting directives are not safe inert characters on that path. `[May-improve]`: emit the text literally, with no format expansion.

For `addregion`, normal named input count is `2 * point count + 1`, including the name below all coordinate values. Points are consumed newest first, each Y then X. Count below 2 errors before point processing; an even count errors after points because no name remains in the declared count. `[Must-match]` The name check rejects nonzero canonical I, but does not consistently reject other non-S tags. Missing or malformed names do not create a defined anonymous-region facility. `[May-improve]`: reject a name operand that is not S with a VM error.

For scroll controls, the first three image positions permit S or I zero; a nonzero I errors, while unrelated non-I tags can slip through and become null images. The fourth image position rejects unrelated types and permits only S, I zero, or I -1. Coordinates are unchecked raw payloads. `[Must-match]` Button graphics has a separate OR-condition bug: **one** valid image operand is enough to admit all three, after which all three are resolved. `[May-improve]`: treat each image operand that is not S as no image. Reply-flag configuration affects option flags, and right-button button callbacks are actually registered on a region of the given name. `[May-improve]`: store reply flags as reply flags, and attach right-button callbacks to the named button.

Callbacks are associated with the calling program and stored procedure indexes; registration does not execute them. The any-key callback takes priority over individual keys; procedure zero can still consume a key without executing a procedure. `[Must-match]` Program removal clears individual-key program bindings, but the inspected cleanup does not explicitly clear the global any-key binding. `[May-improve]`: clear the any-key binding when its owning program is removed. Region configuration/refresh also must not be mistaken for a complete working point-hit-test implementation: the underlying initial region checker is an empty false-returning path, although subsequent pending-region callbacks can still run. `[Must-match]`

`tokenize` takes the delimiter as an unchecked-range I character code. With previous I zero it returns source bytes before the first delimiter/NUL as D, even an empty token. `[Must-match]` With previous S it finds the **first occurrence of that text anywhere in the source**, advances past it, and searches for a delimiter. At that delimiter it measures the following token's length but copies from the delimiter itself: a following token of length n yields the delimiter plus only the first n-1 token characters. `[Must-match]` No further delimiter gives I zero. `[Must-match]` An absent previous substring dereferences a missing match. `[May-improve]`: return I zero. Delimiter zero can walk beyond the string terminator. `[May-improve]`: stop every scan at the source terminator. This is not a corrected conventional tokenizer.

### 3.2 Host Execution State, Waiting, and Incomplete Library Dialogue

The host-busy status bit `0x0020` prevents ordinary interpreter execution while it remains set. Many wrappers set it temporarily and clear it afterward rather than saving/restoring its previous state. There is no universal exception-safe restoration. `[Must-match]` `critter_damage` returns early on null/non-critter without clearing it; `attack` returns when dialogue is active after setting it. These paths can stall the program independently of its stack/result behavior. `[May-improve]`: clear the host-busy bit on every return path of those two wrappers. Self-destruction/kill paths can set execution-cancellation bit `0x0100`, preventing further ordinary execution even though the opcode's nominal stack contract completed. `[Must-match]`

The general library dialogue backend is incomplete in this revision. Reply definition returns success without creating a reply; timed-message display returns -1 and becomes a VM error; dialogue run returns -1, ignored by `sayend`. `[Must-match]` Its creation path indexes a dialogue slot using the initial depth -1 before incrementing depth, an unsafe indexing defect rather than a specified usable initial state. `[May-improve]`: index the new slot only after incrementing the depth. Do not infer a working library dialogue session from the opcode names or from configuration setters that do work. `[Must-match]`

Movie playback installs waiting bit `0x0010`, but the interpreter clears waiting when its predicate returns **true**. The movie predicate returns true when a library dialogue depth is positive, otherwise returns whether the movie is playing. Thus ordinary playback can clear waiting immediately; a predicate polled only after completion can remain false and continue waiting. This is not a reliable "wait until movie finishes" contract. `[May-improve]`: keep the program waiting until playback ends. Game movie playback uses a separate host path. `[Must-match]`

Undefined words `0x807D` and `0x807E`, and unsupported `setglobalmousefunc`, raise VM errors, not diagnostic-only no-ops. Conversely, `animate_jump` and `make_daytime` consume nothing and do not raise that error merely because their intended facility is absent. `barter_offer`/`barter_asking` consume three checked integers and push nothing. The reaction, dice, and contest stubs consume their listed operands and push I zero. `[Must-match]`

### 3.3 Game Dialogue Session Rules

`start_gdialog` consumes the bottommost list-number operand even though it never uses it. The passed object is checked and can trigger prototype validation, but the installed dialogue target is the current script owner. If a talking head is selected, mood is derived from the **previous** dialogue target before the new self target is installed; missing prior target can therefore be unsafe. `[Must-match]` In ordinary flow, dialogue-system entry has already installed self as the target, so the lookup reads self. `[May-improve]`: if no dialogue target exists yet, derive the mood from the new self target. Head -1 avoids that head/reaction lookup. Derived fidget moods are 1 good, 4 neutral, 7 bad; with no head the supplied reaction value is used directly. `[Must-match]`

`gsay_reply`, `gsay_option`, `gsay_message`, and `giq_option` allow S only in their message/text position. All other listed operands, including procedure numbers, require I. The later procedure-name string branches in the option wrappers cannot be reached with ordinary S because earlier validation raises a VM error. Literal text does not remove the message-list operand from the stack contract. `[Must-match]`

Positive dialogue message-list number k selects the dialogue message resource belonging to zero-based script-list entry `k - 1`. It is not a prototype-message category or the current caller's implicit list. The underlying catalog has special negative selector pairs for continuation text, but arbitrary nonpositive list numbers are not safely validated before indexing. `[Must-match]` for positive k. `[May-improve]`: treat any other nonpositive list number as missing text.

Effective IQ for `giq_option` is player intelligence plus Smooth Talker perk rank. Nonnegative threshold q admits values at least q; negative q admits values at most `-q`. A rejected option still consumes all five operands. `gsay_message` ignores its supplied reaction and inserts the host's standard continuation entry with fixed list/message selectors -2/-2, null procedure, and reaction 50. Reply/option selection can trigger speech and script procedures through the host dialogue loop; no synthetic scalar return is appended afterward. `[Must-match]`

`message_str` only passes positive message numbers to its speech-aware catalog lookup. During a talking-head dialogue it starts that message's speech and lip-sync at once, exactly as a displayed reply does (SPEC-006 §3.9, "Speech requests"), but does not return a sound handle. Nonpositive/missing text becomes D `Error`; this wrapper does not expose the catalog helper's special negative continuation selectors. `[Must-match]`

### 3.4 Object and Geometry Boundary Behavior

Creation's explicit script-file number is one-based; -1 means keep the prototype's default script. Zero is not another spelling of -1: it becomes an invalid-looking file index -1 after the subtraction. An explicit replacement script uses category 4 for critters, 3 for items/scenery, 0 otherwise, and receives a fresh owner/object association. Setting visibility to nonzero hides and makes critters nonblocking; showing them clears nonblocking rather than restoring an independently remembered prior blocking state. Unowned destruction erases a map object, while owned destruction first removes its inventory stack; destroying self can detach its script and hide/mark it temporary rather than immediately freeing it in the owned path. `[Must-match]` Script-file number zero producing index -1 is a defect. `[May-improve]`: treat zero like -1 and keep the default script.

Inventory removals attach the removed representative to hardcoded hex 1/elevation 0, not the owner's current position. Multiplicity-returning removal pushes its computed count even when the host removal fails, and null operands return without any push. `destroy_mult_objs` similarly returns the selected inventory count, but an unowned erased object returns zero. The complete six-byte result slot must not be assumed on every early-return path. `[Must-match]` A missing push leaves the caller's stack short. `[May-improve]`: the per-row safe rules push the stated null result instead.

Null is not uniformly safe. In particular, hearing with two non-null objects always reports false due to an inverted guard, while null hearing inputs enter a dereferencing path. Critter inventory query, hit-point adjustment, and multi-destruction also lack wrapper null guards. Pickup's context lookup compares against the wrong failure code. These are recovered unsafe paths, not a contract to manufacture a Boolean or zero result after a memory fault. `[May-improve]`: apply the per-row safe rules in sections 2.5 and 2.8.

Area visibility accepts elevation bounds and numeric hex bounds, not four rectangle coordinates. It sorts elevation bounds, but when the first hex exceeds the last hex both bounds collapse to the last-hex operand (the numerically smaller value) rather than swapping, so only that single hex can match. `[May-improve]`: swap reversed hex bounds. Matching requires a hex between the bounds and `trunc((hex - first hex) / 200) <= trunc(last hex / 200) - trunc(first hex / 200)`. `[Must-match]` The enclosing elevation loop never increments its current elevation: for ordinary ordered bounds it repeatedly scans/redraws the same elevation and does not return. An empty area does not cure the loop. `[May-improve]`: swap reversed hex bounds, and advance the elevation by one after each pass so the call terminates after the last elevation.

`tile_num_in_direction` differs from the underlying directional helper at zero distance: the opcode returns -1. Its orientation check tests only below 6, not nonnegative. Distance and neighbor arithmetic inherit the 200-column parity/border rules and distance-walk defects documented in SPEC-003. Integer tag validation is not geometric validation. `[Must-match]` The missing lower bound on orientation is a defect. `[May-improve]`: return -1 for a negative orientation.

For `rotation_to_tile`, form relative projected DX/DY. If DX is zero, return 0 for negative DY, otherwise 2. Otherwise take the principal polar angle of `(DX,-DY)`, multiply by `180 * 0.3183098862851122`, truncate toward zero, subtract that integer from 90, add 360 if negative, divide by 60 and cap at 5. Unlike object screen-bound queries, this depends on relative coordinates, not a fixed screen rectangle. `obj_on_screen` ignores object-hidden flags and pixel opacity; it uses bounding-rectangle overlap on the current elevation. `[Must-match]`

### 3.5 Stats, Combat Parameters, Variables, and Clock Effects

Roll values 0-3 mean critical failure, failure, success, critical success. `roll_vs_skill` and primary-stat checks store their margin in script context; `how_much` still consumes an ignored I operand when fetching it. Player-stat selectors 0-4 mean unspent skill points, level, experience, reputation, karma. Primary stat selectors 0-6 and skill selectors 0-17 retain the order described in SPEC-003. `[Must-match]`

`has_trait` kind 0 and kind 2 query global player perk/trait state even when a different non-null object was passed. `[Must-match]` Kind 1 object selectors are:

| Selector | Returned attribute |
| ---: | --- |
| 5 | Critter AI packet |
| 6 | Critter team |
| 10 | Orientation |
| 666 | **Visible** Boolean: hidden flag clear, despite the misleading invisibility selector meaning |
| 669 | Total inventory weight |

The selector table is `[Must-match]`, including the inverted meaning of selector 666.

Unsupported selectors normally give zero. Several perk/trait checks have only an upper-bound comparison, so negative indexes are not universally safe. `[May-improve]`: return zero for a negative perk or trait index. Add-trait kind 0 adds a player perk and ignores supplied value; kind 1 sets AI/team. Remove-trait's perk branch does nothing. Their -1 results are not reliable success/failure reports. Stat modification adds to player base value; skill modification adds to unspent points and then buys positive increments. Poison and radiation adjustment act only on the player; positive input first subtracts its resistance percentage, negative removal is not resistance-reduced. `[Must-match]`

LVAR, MVAR, campaign GVAR, per-program stack globals, and exported variables are five separate domains. LVAR lookup can lazily allocate zero-filled slices, but does not independently validate each element against its own slice length after adding its base. `[May-improve]`: treat an element beyond the script's own slice as out of range (reads zero, writes are ignored). System/map-script local reads return -1. Campaign/MVAR out-of-array reads normally return zero; an entirely absent campaign array instead makes `global_var` return -1. Setters consume their arguments but do not push host status. `load_map` directly stores its parameter in campaign element 32: string map lookup stores it even if the name later fails to match; a negative numeric map index does not. `[Must-match]`

Ambient lighting computes requested intensity with integer truncation (formula `[Must-match]`):

- Input exactly 50 requests 40960.
- Input greater than 50 requests `40960 + trunc(input * 24576 / 100)`.
- Other input requests `16384 + trunc(input * 24576 / 100)`.

The host then applies its ambient-light constraints. This has a discontinuity at 50 and is not a single linear percentage conversion. Object lighting separately uses the recovered coefficient 65636. `[Must-match]` `gfade_in`/`gfade_out` use their input only as a nonzero gate; library `fadein`/`fadeout` use it as duration. `[Must-match]`

Animation delays are scheduler frame-step counts, not wall-clock milliseconds or game ticks; negative delay normally waits on previous sequence activity. Registration command 1 accepts request flags such as 1 unreserved, 2 reserved, 4 suppress automatic standing, and 512 insignificant. Command 3 still pops its ignored parameter. Most registrations are suppressed in combat, while registered sound cues are not. Direct `anim` forward playback retains **only** the existing knockdown bit in critter combat status, discarding other bits; reverse playback sets knockdown. Death animations 20-35 can change to their static art numbers 28 higher after forward animation; selectors 1000/1010 remain direction/frame controls. `[Must-match]`

Both eight-operand attack words use self as attacker. Two middle operands are consumed but unused; accuracy and min/max bounds are forwarded for a new combat request. The final supplied pair sets an additional request mode only when its two values are equal, copying them verbatim. During existing combat, only the attacker's initial opponent/maneuver is set if not already marked. Active dialogue can leak the busy bit before a new request. `[May-improve]`: clear the host-busy bit on that return path. The two-object `attack_setup` supplies zero accuracy and range 0-2147483647 instead; it is not the same stack ABI. `[Must-match]`

Mass kill mode 1 cycles static death animations `62,51,52,53,63,62,54,56,59,62,63`, subject to available-art/violence correction; other nonzero modes use 48. Hidden, already-dead, and final-death-art objects are skipped. The repeat/count guard diagnoses rather than defining unlimited destruction. Single kill merely warns during load restoration and proceeds, whereas mass kill refuses it. `[Must-match]`

`metarule` 13 sets end-game request and pushes zero; 14 tests that the map has not been saved/initialized; 15 queues the parameter's elevator and pushes zero; 16 returns party count; 22 returns current loading state. Unknown rule numbers return zero without a generic extension mechanism. `[Must-match]`

One game second is 10 ticks, minute 600, day 864000. HHMM is `100 * (whole elapsed hours modulo 24) + (whole elapsed minutes modulo 60)`. Calendar queries return 1-based month/day using the host's fixed non-leap calendar. Time advance processes events after each full day and once more after the signed remainder, even when that remainder is zero; it is not just a clock assignment. `[Must-match]` Very large, negative, or overflow-prone time values do not acquire a new safe arithmetic contract from these wrappers. `[May-improve]`: perform the arithmetic as wrapping 32-bit two's complement.

Floating message style -2 uses red palette lookup 31744, font 103 and recenters on the player. Other styles use font 101. Style -1 advances a shared color cycle, initially from stored style 1 to style 2, wrapping after 12 to 1. Exact foreground RGB555 lookup indexes are (whole table `[Must-match]`):

| Style | Lookup index |
| --- | ---: |
| 0 normal, 8 yellow, unrecognized style | 32747 |
| 1 black, 5 purple, 10 grey | 10570 |
| 2 red | 31744 |
| 3 green | 992 |
| 4 blue | 31 |
| 6 near-white | 21140 |
| 7 light-red | 32074 |
| 9 white | 32767 |
| 11 dark-grey | 8456 |
| 12 light-grey | 15855 |

### 3.6 Prototype Query Selectors

`proto_data` exposes selected fields, not arbitrary byte offsets or the complete subtype payload. `[Must-match]` Supported scalar selectors are:

| Prototype category | Implemented selectors |
| --- | --- |
| Items | 0 PID; 1 name; 2 description; 3 art; 4 light radius; 5 intensity; 6 basic flags; 7 extended flags; 8 script; 9 subtype; 11 material; 12 size; 13 weight; 14 cost; 15 inventory art |
| Critters | 0-8 as above; 10 head art; not the stat arrays or AI/team payload |
| Scenery | 0-8 as above; 9 subtype; 11 material |
| Walls | 0-8 as above; 9 material |
| Tiles | None in this query helper: zero result |
| Miscellaneous | 0-7 as above, with the description return-tag defect |

The selector table is `[Must-match]`.

Successful names/descriptions are copied to D in the calling program, except miscellaneous description: it returns I containing a raw host text-reference value. That I is not a portable string offset or a usable dynamic-string result. `[May-improve]`: return that description as D like the other categories. Unsupported member and unavailable-prototype paths use the initialized I zero fallback, not an arbitrary memory read. `[Must-match]`

### 3.7 Sound, Movies, and Named-Event Lifetimes

Library sound mode bits are 1 looping, 2 sixteen-bit rather than eight-bit samples, 256 memory-backed, 512 streamed, 4096 mono, 8192 stereo. When conflicting storage/channel bits occur, the later streamed/stereo setting wins. Successful allocation uses the first free slot 0-31 and returns its index combined with `0xA0000000`, as an **I** value even though those payload bits resemble other tags. `[Must-match]`

Handle -1 is an ignored/success-like no-op for library control helpers. Ordinary pause/stop stops a memory-backed sound but pauses a stream; resume restarts memory playback or unpauses the stream. Rewind restarts only an already-playing sound. Delete frees a slot; completion callbacks also clear slots. Invalid handles are not fully range-validated after marker bits are stripped, so arbitrary I values must not be treated as safely rejected. `[May-improve]`: treat a handle whose stripped slot is outside 0-31 or empty as a no-op. None of the control opcodes pushes the helper's success code. `[Must-match]`

Game sound-name effects return uppercase text, not playback handles. Open/container names use action letters O/C/L/N/U for open/close/lock/unlock/use and the prototype's stored sound byte. Character names combine art stem and animation sound letters. Weapon names combine `W`, effect R/A/O/F/H for ready/attack/empty/fire/hit, weapon sound byte, primary/secondary variant, material letter, and `XX1`. The material letter is X except an applicable hit uses M for glass/metal/plastic, W for wood, S for dirt/stone/cement, F otherwise; explosion/plasma/EMP hits retain X. The broken ambient/interface wrappers intend an A/N prefix, a base right-justified to at least six characters, and trailing 1; the item wrapper uses the same N form as interface. Scenery intends S, P for passive selector 1 or A otherwise, action letter, a base right-justified to at least four characters, and 1. `[Must-match]` Wrong input-tag checks and unchecked short destination buffers are retained defects, not permission to accept longer safe names. `[May-improve]`: keep the wrong I tag check, raise a VM error when the base name resolves to null, and never copy a built name into a too-short buffer.

Game movie numbers are 0-13. All request fade-in and paused music. Numbers 0,1,2,5,8,9,11,12 also request fade-out; 3 and 4 add fade-out when the map name is empty. Game playback temporarily disables dialogue background updating and restores it afterward. It is separate from generic movie rectangle/window selection, generic movie option bits, and generic movie waiting. `[Must-match]`

Named events/handlers occupy 40 slots, with case-insensitive names stored in 32-byte name areas. `[Must-match]` There is no safe long-name bounds enforcement. `[May-improve]`: truncate names to 31 characters. Registering an existing name replaces its binding; full-table and missing-name statuses are ignored by the opcodes. Signaling queues hits only for a usable, nonbusy binding. Update dispatches at most one pending hit per binding per pass; one-shot events are freed after dispatch, while persistent handlers remain. Program removal clears their bindings. These callbacks, keyboard callbacks, exported procedures, and script timers are distinct registries with different lifetimes and operands. `[Must-match]`

### 3.8 Player Base Art and Armor Art

This section gathers every rule that sets the player's base critter art, because the opcode rows above depend on it. It also covers the engine paths outside the opcodes. Art identifiers follow SPEC-007 §2.1 and §2.3. Armor's AC, DR, DT and perk effects are in SPEC-011 §3.10. The inventory figure is in SPEC-014 §3.34.

**Jumpsuit indexes.** `[Must-match]`

- At art start-up, the engine scans `art/critters/critters.lst` for the base names `hmjmps` (male jumpsuit) and `hfjmps` (female jumpsuit), compared case-insensitively. A row's base name is its text up to the first space, comma, semicolon, tab or line feed, cut to 12 characters (SPEC-001). The zero-based row number of a match is that gender's jumpsuit index. If a name occurs more than once, the last row wins. A name that never occurs leaves that gender's index at 0.
- In the corpus list (59 rows), the male jumpsuit is row 11, art identifier `0x0100000B`, and the female jumpsuit is row 4, `0x01000004`.

**Default player art.** The engine keeps one default player art index. It is 0 until the first gender update below. The player object is created during object start-up, before the prototype start-up that runs the first gender update, so it briefly carries base index 0. Row 0 of the corpus list is `reserv`. Nothing is drawn between those two steps. `[May-improve]`: create the player directly with the gender's jumpsuit art; never show `reserv` art for the player.

**Gender update.** `[Must-match]` It runs at:

1. every player-sheet load: prototype start-up and reset (both load `premade/player.gcd`), each premade selection on the character selection screen (SPEC-014), and the self-running demo's `premade/combat.gcd`;
2. leaving the character editor with Done in creation mode;
3. the end of loading a saved game;
4. the end of `move_obj_inven_to_obj` when the source owner is the player.

It does the following, in order:

1. Set the default player art to the male jumpsuit index when the player's effective gender stat is 0 (male), otherwise to the female jumpsuit index.
2. If the player wears no armor, replace the player object's art at once, with no animation: critter category, the default player art, animation 0 and selector 0. The weapon code is the player's current art weapon code when either hand holds any item, otherwise 0.
3. If the player wears armor, leave the player object's art unchanged.
4. In both cases, set the player prototype's art to the critter category plus the default player art, with every other field 0.

**Armor art fields.** An armor prototype stores a male and a female art field (SPEC-003). A female wearer (gender stat 1) uses the female field, and any other wearer uses the male field. Only the field's low 12 bits are used, as the base index. The corpus stores complete critter art identifiers there, and no corpus armor stores -1. `[Must-match]`

| Armor PID | Armor | Male field (index, base name) | Female field (index, base name) |
| ---: | --- | --- | --- |
| 1 | Leather Armor | `0x0100000C` (12, `hmlthr`) | `0x01000005` (5, `hflthr`) |
| 2 | Metal Armor | `0x0100000E` (14, `hmmetl`) | `0x01000007` (7, `hfmetl`) |
| 3 | Powered Armor | `0x01000001` (1, `hapowr`) | `0x01000001` (1, `hapowr`) |
| 17 | Combat Armor | `0x0100000A` (10, `hmcmbt`) | `0x01000003` (3, `hfcmbt`) |
| 74 | Leather Jacket | `0x0100000D` (13, `hmmaxx`) | `0x01000006` (6, `hfmaxx`) |
| 113 | Robes | `0x01000002` (2, `harobe`) | `0x01000002` (2, `harobe`) |
| 232 | Hardened Power Armor | `0x01000001` (1, `hapowr`) | `0x01000001` (1, `hapowr`) |
| 239 | Brotherhood Armor | `0x0100000A` (10, `hmcmbt`) | `0x01000003` (3, `hfcmbt`) |
| 240 | Tesla Armor | `0x0100000E` (14, `hmmetl`) | `0x01000007` (7, `hfmetl`) |

**Inventory window.** `[Must-match]`

- When the player inventory window closes and its top-level owner is the player, the player object's art is replaced at once, with no animation, by the figure art of SPEC-014 §3.34:
  - the base is the worn armor's field for the player's gender, or the default player art when no armor is worn or the field is -1;
  - the weapon code is that of the item in the interface bar's active hand when it is a weapon, otherwise 0;
  - animation and selector are 0.
- Taking armor off in the window (dragging it into the item list, dropping it on the ground or into a container) therefore restores the jumpsuit when the window closes. Putting armor on, or swapping it, shows the new armor's art from then on. The art does not change while the window is open; only the figure does.
- The loot, barter and use-on windows never apply the figure art to the player object.
- With no armor worn, the reference uses the default player art when the owner's prototype lookup succeeds. When the lookup fails, it reads the base from an invalid prototype. `[May-improve]`: use the default player art in both cases.

**`wield_obj_critter` with armor.** `[Must-match]`

1. The previously worn armor, if any, loses its worn flag but stays in the inventory. The new armor gains the worn flag.
2. The base is the new armor's field for the wearer's gender. A field of -1 gives base index 1, not the jumpsuit (row 1 is `hapowr` in the corpus). `[May-improve]`: use the default player art. This fallback is unreachable with corpus armor.
3. For the player, an art change is registered in a new reserved animation sequence, with start delay 0. The new art is the critter category, that base, animation 0, and the player's current weapon code. The requested selector is facing plus 1, and the standing animation encodes it as 0 (SPEC-007 §2.3). The opcode then applies the armor change of SPEC-011 §3.10.
4. For any other critter, the art and protection are unchanged; only the worn flag moves.

**Taking armor off by script.** `[Must-match]`

- `rm_obj_from_inven` on a worn item, when the owner is the player: once the item is removed and placed on the map (row `0x80D9`), the interface bar's item slots are refreshed and the armor's effects are removed (SPEC-011 §3.10). The player object's art then becomes the default player art, keeping its current animation, weapon code and selector. The change is applied at once, with no animation, and the screen is refreshed. For any other owner, the art is unchanged.
- `move_obj_inven_to_obj` from the player moves everything, removes the old armor's effects (SPEC-011 §3.10), and then runs the gender update. The hands are now empty, so the player shows the jumpsuit with weapon code 0.

**Worked example.** `[Must-match]` A female player (gender stat 1) with Leather Armor worn and an empty active hand closes the inventory window. Her art becomes `0x01000005` (`hflthr`, animation 0, weapon code 0). She then drags the armor back into the item list, puts a Spear (weapon code 4) in the active hand, and closes the window again. Her art becomes `0x01004004`: base 4 (`hfjmps`) plus weapon code 4 times `0x1000`. A male player in the same steps gets `0x0100000C`, then `0x0100400B`.

## 4. Synthetic Test Fixtures (Hex / Text)

These are three independent bytecode **fragments**, not complete compiled-script files. Begin with empty value/return stacks, enter at fragment byte zero, and stop exactly at its end; do not execute unspecified following bytes. The harness supplies the stated script context, string storage and mock hosts. No real GUI, sound, map transition, file picker or game object is created. Opcode words and immediate payloads are big-endian; instruction boundaries need not coincide with dump lines. Every expected result below exercises `[Must-match]` behavior; no fixture depends on a reference defect or on a `[May-improve]` safe rule.

### 4.1 Fixture A: Setter Order, Context Reference, and Clock Results

Length 30 bytes (`0x1E`). Mock campaign array contains element 4, initial value 7. Current self is opaque reference `0x00010000`; that object's elevation is 1. Clock queries both observe 12340 ticks.

```text
0000: C0 01 00 00 00 04 C0 01 00 00 00 63 80 C6 C0 01
0010: 00 00 00 04 80 C5 80 BC 80 EC 80 EA 80 EB
```

| Executed host word | Pops, first to last | Mock response / effect | Stack afterward, bottom to top |
| --- | --- | --- | --- |
| `80C6` | I 99, I 4 | Campaign element 4 becomes 99; no return | Empty |
| `80C5` | I 4 | I 99 | I 99 |
| `80BC` | None | I reference `0x00010000` | I 99, I reference |
| `80EC` | I reference | I 1 | I 99, I 1 |
| `80EA` | None | I 12340 | I 99, I 1, I 12340 |
| `80EB` | None | I 1234 | I 99, I 1, I 12340, I 1234 |

Expected final value-stack bytes:

```text
0000: 00 00 00 63 C0 01 00 00 00 01 C0 01 00 00 30 34
0010: C0 01 00 00 04 D2 C0 01
```

The return stack remains empty. Inserting a setter-status zero would shift every later expected result and is incorrect.

### 4.2 Fixture B: Sound Handle Tag and Statement-Only Controls

Length 28 bytes (`0x1C`). Install static string reference 6 as `beep`. Mock sound slots 0 and 1 are already occupied; slot 2 is free. Sound start succeeds in looping memory-backed mode 1 and returns integer-tagged payload `0xA0000002`.

```text
0000: 90 01 00 00 00 06 C0 01 00 00 00 01 80 98 80 1B
0010: 80 99 80 1B 80 9A 80 1B 80 9B 80 9D
```

`801B` duplicates one complete tagged value, not just its payload. Expected host trace:

1. `8098` pops I mode 1, then S reference 6; starts `beep` and pushes I handle `0xA0000002`.
2. Duplicate, then `8099` consumes one handle and pauses the sound. For this memory mode that is a backend stop; one original handle remains.
3. Duplicate, then `809A` consumes one and restarts memory playback; one remains.
4. Duplicate, then `809B` consumes one and performs the same pause/stop operation as `8099`; one remains.
5. `809D` consumes the last handle, deletes sound and frees slot 2.

Both stacks finish empty. The returned handle is I even though its payload begins with `A0`; its stack tag is `C0 01`, not `A0 01`. Helper success codes are not pushed by the four control operations.

Type-error mutation: change only the immediate tag at offset `0x0006` from `C0 01` to `A0 01`, leaving payload bytes unchanged. `8098` consumes both tagged inputs, rejects the float mode, raises a VM error, and performs no sound-host operation. Stack remains empty and ended/error low status bits are `0x0005`; the following duplicate does not execute.

### 4.3 Fixture C: Animation End Selector and Game Dialogue Text

Length 126 bytes (`0x7E`). Mock host is outside combat; current script owns live critter reference `0x00010000`, with a valid prototype other than the special violence-filter PID. Install static reference 6 as `Hello` and reference 14 as `Continue`. Request flags 1 are unreserved. Game dialogue start/exit and animation registration succeed; the mock dialogue loop returns without executing an option callback.

```text
0000: C0 01 00 00 00 01 C0 01 00 00 00 01 81 0E C0 01
0010: 00 01 00 00 C0 01 00 00 00 01 C0 01 FF FF FF FF
0020: 81 0F C0 01 00 00 00 03 C0 01 00 00 00 00 81 0E
0030: C0 01 00 00 00 0A C0 01 00 01 00 00 C0 01 00 00
0040: 00 00 C0 01 FF FF FF FF C0 01 00 00 00 00 80 DE
0050: 81 1C C0 01 00 00 00 0A 90 01 00 00 00 06 81 1E
0060: C0 01 00 00 00 0A 90 01 00 00 00 0E C0 01 00 00
0070: 00 02 C0 01 00 00 00 32 81 1F 81 1D 80 DF
```

| Host word | Pop trace / mock observation |
| --- | --- |
| First `810E` | I parameter 1, I command 1: begin unreserved animation registration |
| `810F` | I delay -1, I animation 1, I object reference: queue walk animation |
| Second `810E` | I ignored parameter 0, I command 3: end registration; no separate end opcode |
| `80DE` | I background 0, I head -1, I reaction 0, I object reference, I unused list 10: install self dialogue target, no head lookup |
| `811C` | No pops: start game dialogue, mock status success |
| `811E` | S `Hello`, I list 10: set literal reply |
| `811F` | I reaction 50, I procedure 2, S `Continue`, I list 10: add literal-text option bound to procedure number 2 |
| `811D` | No pops: run mock dialogue, no selection/callback |
| `80DF` | No pops: successful exit clears dialogue target/script association |

Both stacks finish empty, and no statement has pushed an implicit status. Host-busy state is clear afterward. Omitting the ignored parameter before command 3 would cause the end-registration instruction to pop an unrelated value or underflow. Using S for procedure 2 would be a type error even though the message itself accepts S.

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings

Update notes (2026-10-22): the `world_map` and `town_map` rows now say what each request opens (SPEC-014 §3.39), checked against the reference request dispatcher. `tools/spec_lint.py` gives 0 errors.

Update notes (2026-10-23): the `message_str` paragraph now says that, during a talking-head dialogue, the opcode starts that message's speech and lip-sync at once, as a displayed reply does (SPEC-006 §3.9). No tag change. `tools/spec_lint.py` gives 0 errors.

Update notes (2026-10-23): section 2.2 gains the registry-lifetime table. The whole exported-variable registry is cleared on every map load and game reset, never per program; procedure entries belong to a program instance and go with it. It also gives re-export across maps and on one map, the corpus export survey, import resolution timing, and the `0x8009` ordering (`[May-improve]`). The `0x80C7` row points to the SPEC-004 §3.7 table, and the `0x810A` row to SPEC-014 §3.46. Net tags: +6 `[Must-match]`, +1 `[May-improve]`. `tools/spec_lint.py` gives 0 errors.
