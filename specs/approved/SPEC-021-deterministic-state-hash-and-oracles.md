# SPEC-021: Deterministic State Hash, Combat Sub-Hash, and Trace Oracles

**Status:** approved by the Spec-Author sub-agent (Team A), 2026-10-27, under AGENTS.md "Spec Authoring (Team A)". Revision 4 (Team A, 2026-10-30); see "Revision history".

**Provenance.** Author: the Spec-Author sub-agent (Team A), 2026-10-27.
Sources:

- the owner's brief for this spec, relayed by the coordinator on 2026-10-27:
  a deterministic 64-bit `WorldState::state_hash(&self) -> u64` over entity
  handles (index and generation), hex coordinates, orientations, HP/AP,
  inventory counts, the RNG state, the combat turn queue and the GVAR, MVAR
  and LVAR script variables; a canonical byte stream with fixed field order,
  widths, endianness, absent-value encoding and sorted collections; a public,
  fully specified hash function such as FNV-1a 64, and never std's `Hash` or
  `DefaultHasher`; a separate combat sub-hash; two TSV trace formats (a VM
  opcode execution trace and a seeded combat turn-by-turn `state_hash` trace)
  with an `asset-audit` command to dump them; goldens in
  `tests/fixtures/spec021/`, not in `oracle/`; the determinism and single-bit
  tests; hand-checkable synthetic vectors; tags and a Decisions section;
- the public FNV-1a hash definition by Fowler, Noll and Vo, as published by
  its authors and in the IETF Internet-Draft "The FNV Non-Cryptographic Hash
  Algorithm" (draft-eastlake-fnv), and its published 64-bit test values;
- the project's approved specs: SPEC-003 (object identifiers, hex grid,
  orientation, elevation, script identifiers, MVAR/LVAR arrays), SPEC-004
  (VM stacks, instruction words and the section 4.1 program), SPEC-005
  (GVAR/MVAR/LVAR words), SPEC-006 section 3.4 (the random generator) and
  section 4.4 (its seed vector), SPEC-007 section 3.3 (combat partitions and
  turns), SPEC-020 (session runtime and the re-recording rule it uses);
- the project's documentation of its command-line tools (`asset-audit`,
  `cargo xtask fixtures`, `--seed`) in `docs/REFERENCE.md`, and the fixture
  READMEs under `tests/fixtures/`.

No decompilation, disassembly or re-implementation source was consulted, no
game asset was opened, and no engine Rust code was read: engine types are
named here only by the names the brief gives (`WorldState`, the combat state,
the RNG). Team A's general knowledge comes from model training data whose
sources cannot be audited (see `PROVENANCE.md`, "In-session spec authoring").
This spec defines an engine-side determinism oracle; it adds no game rule.

**Provenance of revision 2.** Author: the Spec-Author sub-agent (Team A),
2026-10-27, on the coordinator's request relaying the implementer's open items
4 and 5. Sources: the implementer's report as relayed by the coordinator (the
engine stores every inventory item as an entity with its own handle; its
extraction test built handles `1:1`, `2:3` and an item `3:1`; the synthetic
combat map is `maps/synthetic.map`; the synthetic corpus has no `data/ai.txt`,
so the NPC's turns took no action); SPEC-009 sections 2.2 to 2.4, 3.1, 3.5,
3.6, 3.8, 3.12, 3.14, 3.17 and 3.19 (the `ai.txt` grammar, packet keys and
NPC turn); SPEC-007 sections 3.3 and 3.5 (combat start, turn order and hit
chance); SPEC-002 sections 2.5 and 3.5 (`.MSG` records); SPEC-014 section 3.1
(the default language); SPEC-020 section 3.4 (the re-recording rule). No
engine Rust code was read for this revision either.
Every revision 2 vector was recomputed from the spec, and then checked a
second time from the written fixture bytes alone (section 7).

**Provenance of revision 3.** Author: the Spec-Author sub-agent (Team A),
2026-10-27, on the coordinator's request relaying the implementer's Wave 4
finding on section 3.9. Sources:

- the implementer's report as relayed by the coordinator, recorded in section
  5.8 as measured fact;
- SPEC-001 sections 2.2, 2.3, 3.3 and 3.4 (the FRM header, frame records,
  direction sequences and timing);
- SPEC-007 sections 2.1 to 2.3 (the art identifier, critter suffixes and
  directional fallback), 3.3 (turns and stand-up), 3.4 (attack AP debit), 3.6
  (punch-to-kick substitution and critical-failure table 0), 3.8 (death and
  fall choice) and 3.10 (attack sequences, the start rule, registration
  failure and reactions);
- SPEC-009 sections 3.9 and 3.12 (weapon switching, the unarmed upgrade and
  the attack step);
- a listing of the files that `cargo xtask fixtures` writes under
  `art/critters/` and `proto/critters/` in its synthetic corpus. Team A read
  the one-row critter list `sample,0`, the three critter FRM headers, and the
  art identifier word of the corpus's only critter prototype (`0x01000000`).
  No fixture-builder code was read.

No engine Rust code, decompilation or game asset was used. The new art files
were written from SPEC-001 and checked a second time by a separate parse of
the written bytes (section 7).

**Provenance of revision 4.** Author: the Spec-Author sub-agent (Team A),
2026-10-30, on the owner's brief of 2026-10-30, "Tick-by-Tick Replay State-Hash
Checkpoints". Sources:

- the owner's brief, which names this work revision 3 and the folder
  `tests/fixtures/spec021/rev3/`. Revision 3 of this spec was already approved,
  so this is revision 4, and the folder keeps the brief's name (D-40);
- SPEC-020 section 2.3 (the session façade), SPEC-022 section 3.13 (what
  drawing never changes), SPEC-008 sections 3.4 and 3.5 (world travel and the
  encounter check), and this spec's sections 3.1, 3.7 and 3.8;
- `docs/REFERENCE.md`: the input log, its records and the replay digest, the
  map preview's controls, the world map's controls, and the combat keys;
- the fixture READMEs under `tests/fixtures/`, and a listing of the files that
  `cargo xtask fixtures` writes;
- **a black-box measurement by Team A on 2026-10-30.** Team A ran the built
  `vault-platform` binary on that synthetic corpus, in private copies under
  `target/`, with hand-written input logs, to confirm that each scenario of
  section 3.11 can run. It read only what a user of the binary sees: the
  digest line on stdout, the diagnostics on stderr (including the field names
  in a parse error), and the frames `--replay-frame` writes. Its findings are
  labelled **measured by Team A** in sections 3.11 and 5.10. The two committed
  logs are Team A's own input, and no output of those runs is committed (D-49).

No engine Rust code and no fixture-builder code was read, and no decompilation,
disassembly, re-implementation source or game asset was used. The format
vectors were computed from this text, then checked by a second, separate script
from the written files alone (section 7).

### Revision history

- **Revision 1 (2026-10-27).** First approved text.
- **Revision 2 (2026-10-27).** Resolves the implementer's two open items.
  - **Open item 4: W0 held an item that had no entity.** The engine keeps
    every inventory item as an entity, so no world could produce W0. W0 now
    also holds the item as entity `3:1`, with no position (section 5.2). W1 to
    W4, the mutation rows and the combat trace example are rebased on the new
    W0. Their combat streams do not change, because the item is not a combat
    participant. The revision 2 vectors are in `tests/fixtures/spec021/rev2/`
    (section 3.9). Section 2.4 states how a held item appears, and sections
    2.4 and 3.2 add an order for two entity records with the same handle.
  - **Open item 5: the seeded combat golden had no AI.** `rev2/ai.txt` is a
    one-packet SPEC-009 file that makes the NPC fight on its turns, and
    `rev2/combatai.msg` is a one-line taunt catalog (section 5.7). The combat
    trace setup now installs both in the corpus (section 3.7), the dump refuses
    to run when the AI does not initialize, and T11 checks that the NPC acts.
    The seeded golden must be re-recorded with the AI loaded (section 3.9).
  - The stream format, the magic `VSH1`/`VCH1`, the trace formats and every
    revision 1 hash of a combat stream are unchanged.
- **Revision 3 (2026-10-27).** Fixes the blocker the implementer hit in Wave 4
  while carrying out section 3.9.
  - **The finding.** With the AI fixture installed, the NPC still spent no AP
    in the seeded trace. Its attack request was refused because the attack's
    animation sequence could not register: the corpus has no punch art for the
    NPC's art base `sample` (section 5.8).
  - **The fix.** Four synthetic 75-byte critter FRMs for base `sample` go in
    `tests/fixtures/spec021/rev2/`: punch (`sampleaq.frm`), hit flinch
    (`sampleao.frm`), fall back (`sampleba.frm`) and stand up
    (`samplecj.frm`). T11's setup installs them under `art/critters/` next to
    the AI fixture (section 3.7). The punch alone is not enough: a punch that
    hits also registers the defender's reaction, and each reaction animation's
    art must load too (sections 5.7 and 5.8).
  - Sections 3.7, 3.9, 4 (T11), 5.6, 5.7, 6 and 7 are updated, and section
    5.8 is new. No byte of any revision 2 vector changes, because animation
    and art are not covered by the hash (section 3.1). The shared synthetic
    corpus and `cargo xtask fixtures` are unchanged.
- **Revision 4 (2026-10-30).** Adds tick-by-tick replay checkpoints.
  - **Checkpoints.** One row per headless step, written after the simulation
    and before rendering (section 3.10).
  - **The stream.** A versioned TSV of `tick`, `input_index`, `state_hash` and
    `combat_hash` or `-`, compared row by row, stopping at the first difference.
    It is the synchronization unit for future lockstep multiplayer (SPEC-025).
  - **Three replay scenarios** on existing synthetic fixtures: exploration and
    dialogue, world-map travel, and seeded multi-turn combat (section 3.11).
  - The world and combat streams, the magics `VSH1` and `VCH1`, the coverage of
    section 3.1, the two trace formats and every existing vector and golden are
    unchanged. New files are in `tests/fixtures/spec021/rev3/` (section 3.12).

## Clean-Room Rules for Research Agents
- Document ONLY binary layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from any inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This spec defines:

1. a canonical byte stream over the covered parts of the world state, and the
   64-bit **state hash** of that stream (section 2);
2. a **combat sub-hash** over the combat state alone (section 2.7);
3. how the engine obtains each covered value, and the rules that keep the
   hash stable (section 3);
4. two TSV trace formats, a **VM opcode trace** and a **combat hash trace**,
   and the `asset-audit` commands that write them (sections 3.5 to 3.8);
5. the tests the implementer must write (section 4);
6. synthetic vectors and golden traces (section 5, and
   `tests/fixtures/spec021/`);
7. a versioned **replay checkpoint stream**, recorded once per headless step,
   and three replay scenarios that use it (sections 3.10 to 3.12; revision 4).

The hash is a determinism oracle for this engine: it lets a test, a replay or
a trace show that two runs reached the same covered state. It is not a save
format, it is never parsed back, and it is not a security hash.

**Compatibility tags.** The original game has no such hash, so in this spec
the tags compare an implementation with **this spec and its fixtures**:

- `[Must-match]`: the byte stream, the hash values, the trace text and the
  command behavior must be reproduced exactly. A golden file that disagrees
  is a failure, not a re-recording.
- `[May-improve]`: an implementation freedom. The implementer may choose
  differently (for speed, memory or structure) only if no output byte of this
  spec changes.
- `[Safe rule]`: how the implementation handles a state the format does not
  expect (a stale handle, an oversized collection, an out-of-range value). It
  encodes a stated value or refuses with an error; it never panics, reads out
  of bounds, or skips the rest of the stream silently.

**Notation.** Byte offsets are zero-based and written in hexadecimal with a
`0x` prefix. "u8", "u32" and "i32" are defined in section 2.2. A handle is
written `index:generation` in decimal, for example `2:3`. Hash values are
written as 16 uppercase hexadecimal digits.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Hash Function: FNV-1a 64

The hash of a byte stream B is FNV-1a with 64-bit parameters. `[Must-match]`

| Parameter | Value |
| --- | --- |
| Offset basis | `0xCBF29CE484222325` (decimal 14695981039346656037) |
| FNV prime | `0x00000100000001B3` (decimal 1099511628211) |
| Arithmetic | Unsigned 64-bit, modulo 2^64 |

Start with H equal to the offset basis. For each byte b of B, in stream
order: set H to H XOR b, then set H to H times the prime, modulo 2^64. The
result is the final H. The empty stream hashes to the offset basis.
`[Must-match]`

Known answers, from the public FNV test values: the empty input gives
`CBF29CE484222325`, the one byte `61` ("a") gives `AF63DC4C8601EC8C`, and the
six bytes `66 6F 6F 62 61 72` ("foobar") give `85944171F73967E8`.
`[Must-match]`

**Single-byte property.** XOR with a byte and multiplication by an odd number
modulo 2^64 are both bijections of the 64-bit state. So two streams of equal
length that differ in exactly one byte always have different FNV-1a hashes.
Every covered scalar in section 2 has a fixed width, so flipping one bit of
one covered field changes exactly one byte of the stream and **always**
changes the hash; section 4's single-bit test relies on this. Changes to more
than one byte can collide, with a probability near 2^-64 for unrelated
inputs. FNV-1a is not collision-resistant against a chosen input, and must
not be used where an adversary picks the state.

The hash is computed over the bytes alone. It must not use std's `Hash`
trait, `Hasher`, `DefaultHasher`, `RandomState`, a third-party hasher whose
output the crate does not document as stable, or any per-process seed.
`[Must-match]` The implementation may feed bytes into a streaming FNV-1a
state instead of building the whole stream in memory first: both give the
same value. `[May-improve]`

### 2.2 Primitive Encodings

Every multi-byte number in the stream is **little-endian**. The stream has no
padding, alignment bytes or length prefix of its own. `[Must-match]`

| Name | Bytes | Encoding |
| --- | ---: | --- |
| u8 | 1 | One unsigned byte |
| u32 | 4 | Unsigned 32-bit integer, little-endian |
| i32 | 4 | Signed 32-bit two's-complement integer, little-endian |
| tag | 4 | Four ASCII bytes, exactly as written in this spec (for example `ENTS` is `45 4E 54 53`) |
| opt(T) | 1, or 1 + size of T | Absent: the single byte `00`. Present: the byte `01`, then T |
| seq(T) | 4 + elements | A u32 element count N, then exactly N elements of T, in the order section 3.2 gives |
| handle | 8 | u32 slot index, then u32 generation |
| position | 8 | u32 elevation, then u32 linear hex number |

These rules are `[Must-match]`:

- No value is encoded with a platform-dependent width: `usize`, `isize`,
  pointers, `f32` and `f64` never appear in the stream.
- A value the engine stores as a signed integer is written as i32. A value it
  stores unsigned is written as u32. Where this spec says u32 for a value the
  engine keeps signed (or the reverse), the 32 stored bits are written
  unchanged; no value is clamped, sign-extended or range-checked by the
  encoder.
- A present value that happens to be zero is still present: `01` and four
  zero bytes, never `00`.
- The linear hex number is `column + 200 * row` (SPEC-003), on the elevation
  the position names. Elevation is a separate field, never folded into the hex
  number or packed with the orientation.

### 2.3 World Stream Layout

The world stream is the concatenation of these parts, in this order.
`[Must-match]`

| Order | Bytes | Content |
| ---: | --- | --- |
| 1 | 4 | Tag `VSH1`: stream magic and format revision 1 |
| 2 | 4 | Tag `ENTS` |
| 3 | variable | seq(entity record), section 2.4 |
| 4 | 4 | Tag `RNGS` |
| 5 | 136 | RNG record, section 2.5 |
| 6 | 4 | Tag `CMBT` |
| 7 | variable | opt(combat record), section 2.6 |
| 8 | 4 | Tag `GVAR` |
| 9 | variable | opt(seq(i32)): the campaign GVAR array, section 2.8 |
| 10 | 4 | Tag `MVAR` |
| 11 | variable | opt(seq(i32)): the current map's MVAR array, section 2.8 |
| 12 | 4 | Tag `LVAR` |
| 13 | variable | seq(LVAR slice record), section 2.8 |

The stream ends after part 13. There is no terminator. `state_hash` is the
FNV-1a 64 hash of this stream. `[Must-match]`

A later revision that changes what the stream covers or how it is encoded
must change the magic (`VSH2`, and so on) and re-record every golden that
contains a state hash. `[Must-match]`

### 2.4 Entity Record

One record for each live entity in the world's generational entity store
(section 3.1). `[Must-match]`

| Record-relative offset | Bytes | Field |
| --- | ---: | --- |
| `+0x00` | 8 | handle: slot index, generation |
| `+0x08` | 4 | u32: prototype identifier (PID) word, as stored |
| `+0x0C` | 1 or 9 | opt(position): present when the entity is placed on the map; absent when it is held in an inventory or is otherwise not on any hex |
| next | 4 | u32: orientation, normally 0-5 (northeast, east, southeast, southwest, west, northwest) |
| next | 1 or 5 | opt(i32): current hit points |
| next | 1 or 5 | opt(i32): current action points |
| next | variable | seq(inventory entry): the entity's direct inventory |

The HP and AP fields are present **exactly when** the PID's type byte (its
most-significant byte, SPEC-003) is 1, a critter. For every other type both
are absent (`00`). `[Must-match]`

An inventory entry is 8 bytes: u32 item PID, then u32 quantity (the stack's
multiplicity). Only the entity's direct inventory is listed. A container held
in that inventory contributes one entry; its own contents are covered only if
the entity store holds that container as an entity, through its own record
(Decision D-14). An entity with an empty inventory, or none, writes count 0.
`[Must-match]`

**Held items (revision 2).** When the store keeps an inventory item as an
entity of its own, as this engine does for every item, that item is covered
twice: once as an entry (item PID, quantity) in its holder's inventory, and
once as its own entity record. The record has the item's handle and PID, its
position is absent (`00`), its orientation is written as stored, its HP and AP
are absent (its PID type is not 1), and it lists its own direct inventory
(count 0 unless it is a container holding items). A stack of N units held as
one entity is one record, and the holder's entry carries quantity N. The
record carries no owner field; the holder's entry links the two only by PID.
`[Must-match]` Section 5.2's W0 shows a held item.

**Equal handles.** A live store never holds two entities with the same handle,
but a snapshot built from plain data can (T4's single-bit flips build some).
Two entity records with the same slot index and generation are ordered by the
rest of their encoded bytes (from record offset `+0x08` on), compared as
unsigned bytes, with a shorter record that is a prefix of the other first.
Records with equal bytes are identical, so their order does not matter. This
makes the stream independent of the supply order in every case. `[Safe rule]`

### 2.5 RNG Record

The random generator of SPEC-006 section 3.4, as 34 i32 values, 136 bytes.
`[Must-match]`

| Record-relative offset | Bytes | Field |
| --- | ---: | --- |
| `+0x00` | 4 | i32: the recurrence state N (the last value Q the recurrence produced) |
| `+0x04` | 4 | i32: the shuffle selector, as stored: the raw last output U, **not** reduced modulo 32 |
| `+0x08` | 128 | 32 x i32: shuffle entries 0 to 31, in index order |

If the engine keeps any further generator state (a draw counter, a cached
seed), it is not covered. The host entropy that the ambient seed `-1` draws
from is not covered either; only its effect on the three fields above is.

### 2.6 Combat Record and Participant Record

The `CMBT` part is opt(combat record): absent (`00`) when no combat is active,
present when the combat state's active flag is set. `[Must-match]`

| Record-relative offset | Bytes | Field |
| --- | ---: | --- |
| `+0x00` | 4 | u32: round counter: the number of complete rounds since this combat started (0 during the first round) |
| `+0x04` | 1 or 5 | opt(u32): current turn: the zero-based position in the active list of the actor the combat state names as current; absent when it names none |
| next | variable | seq(participant record): the active list, in its current turn order |
| next | variable | seq(participant record): the waiting list, in its current list order |

A participant record is: `[Must-match]`

| Offset | Bytes | Field |
| --- | ---: | --- |
| `+0x00` | 8 | handle |
| `+0x08` | 1 | u8 live flag: `01` when the handle resolves to a live entity, `00` when it does not |
| `+0x09` | 0 or variable | Only when live: i32 current HP, i32 current AP, opt(position), u32 orientation |

These are the same values the entity's own record holds. A participant whose
PID is not a critter's (which SPEC-007 does not produce) writes HP and AP as
i32 zero. `[Safe rule]`

### 2.7 Combat Sub-Hash Stream

The combat stream is the tag `VCH1` (combat stream magic, revision 1)
followed by the world stream's parts 6 and 7 unchanged: the tag `CMBT` and
opt(combat record). `combat_hash` is the FNV-1a 64 hash of the combat stream.
`[Must-match]`

So the world stream embeds exactly the bytes the combat stream hashes, after
its own first eight bytes. With no combat active, the combat stream is the
nine bytes `56 43 48 31 43 4D 42 54 00` and `combat_hash` is the constant
`03A3252340D67E8D` (section 5.1). `[Must-match]`

### 2.8 Script Variable Parts

- **GVAR.** opt(seq(i32)): every campaign global variable, in index order 0
  to N-1. Absent only when the campaign array itself is absent (the state in
  which SPEC-005 `global_var` returns -1). `[Must-match]`
- **MVAR.** opt(seq(i32)): the current map's map variables (SPEC-003 header
  array, SPEC-005 `map_var`), in index order. Absent when no map is loaded.
  A loaded map with zero map variables writes `01` and count 0. `[Must-match]`
- **LVAR.** seq(LVAR slice record), one record per script instance whose
  local-variable slice holds at least one element. An LVAR slice record is a
  u32 script identifier (the map instance's script-identifier word, SPEC-003,
  category in its most-significant byte), then seq(i32) of the slice's
  elements in index order. A script instance with no slice, or an empty one,
  writes no record. `[Must-match]`

Per-program VM stack globals, exported VM variables and the VM stacks are not
script variables in this sense (SPEC-004 section 3.1, SPEC-005) and are not
covered.

## 3. Behavioral Rules & Edge Cases

### 3.1 What the Hash Covers, and Where Each Value Comes From

Each covered value is read from the authoritative simulation state that
`WorldState` owns, never from a presentation copy (SPEC-020 section 2.1).
`[Must-match]`

| Covered item (owner's brief) | Source |
| --- | --- |
| Entity handles | Every live key of the world's generational entity store, as slot index and generation. If the handle type exposes only a packed value, the implementer splits it by the store's documented layout and records how in the implementation notes |
| Hex coordinates | The entity's elevation and linear hex number |
| Orientations | The entity's facing |
| HP/AP | The critter's current hit points and current action points, as the world stores them for that critter |
| Inventory counts | The entity's direct inventory: item PID and quantity per entry |
| RNG state | The SPEC-006 generator that gameplay draws from |
| Combat turn queue | The combat state: round counter, current turn, active list and waiting list |
| Script variables | Campaign GVAR, current-map MVAR, and every script instance's LVAR slice |

The entity record also carries the PID word (Decision D-12), so that the
critter rule of section 2.4 and the inventory entries are self-describing.

Everything else is **not covered** by revision 1 (Decision D-19), including:
the game clock and the timed-event queue; animation sequences and their
timers; light levels and object flags; prototype data; the player's
statistics, skills, perks and traits; per-critter combat bookkeeping other
than section 2.6 (free movement, last opponent, accumulated damage, maneuver
bits, the pending attack descriptor); world-map travel state; the VM's
programs, stacks and export registries; camera, UI, cursor and audio state;
and the game profile. A change to an uncovered value leaves the hash
unchanged. `[Must-match]`

### 3.2 Ordering Rules

Every collection is written in one of two orders. Neither may depend on
insertion order into a map or set, on hash-map iteration order, or on memory
addresses. `[Must-match]`

1. **Sorted collections**, sorted ascending by a stable key before encoding.
   The encoder sorts them itself, so a caller may supply them in any order:
   - entity records: by slot index, then by generation (both as u32), then,
     for equal handles only, by the rest of the record's bytes (section 2.4,
     "Equal handles");
   - inventory entries: by item PID, then by quantity (both as u32); equal
     entries are byte-identical, so their relative order does not matter;
   - LVAR slice records: by script identifier as u32; two records with the
     same identifier (which SPEC-003's duplicate-identifier rule can leave
     behind) are ordered by their element sequences, compared element by
     element as i32, a shorter prefix first.
2. **Semantically ordered sequences**, written in the order that has
   gameplay meaning, never re-sorted:
   - GVAR and MVAR elements, and each LVAR slice's elements, by index;
   - RNG shuffle entries by index 0 to 31;
   - the active list in turn order, and the waiting list in list order
     (SPEC-007 section 3.3 scans waiters in list order, so that order is
     state).

Sorting by handle makes the slot allocator's history part of the state. That
is intended: two runs that allocate in different orders have diverged.

### 3.3 Interface

These are the names the implementation exposes. A name may change only if it
collides with an existing item, and the implementation notes then record the
new name. `[Must-match]` for the values; the names are a Team A default
(Decision D-26).

- `WorldState::state_hash(&self) -> u64`: the world stream's hash.
- `WorldState::combat_hash(&self) -> u64`: the combat stream's hash. It is a
  method of `WorldState`, not of the combat state, because participant
  records read entity data.
- `WorldState::state_snapshot(&self) -> StateSnapshot`: the covered values as
  plain data (handles as two u32, PIDs, positions, and so on), with no
  reference into the world.
- `StateSnapshot::canonical_bytes(&self) -> Vec<u8>` and
  `StateSnapshot::canonical_combat_bytes(&self) -> Vec<u8>`: the two streams.
- `StateSnapshot::state_hash(&self) -> u64` and
  `StateSnapshot::combat_hash(&self) -> u64`.
- `fnv1a64(bytes: &[u8]) -> u64`: section 2.1, public so that trace tools and
  tests share one implementation.

For every world W, `W.state_hash()` equals
`fnv1a64(&W.state_snapshot().canonical_bytes())`, and likewise for the
combat hash. `[Must-match]` The `WorldState` methods may hash without building
the snapshot. `[May-improve]`

Computing either hash takes `&self`, draws no random number, allocates no
handle and changes no state. Two calls with no change between them return the
same value. `[Must-match]`

### 3.4 Safe Rules

- **Stale handles in the combat lists.** A handle that no longer resolves
  writes live flag `00` and nothing more (section 2.6). It does not panic and
  is not dropped from the list. `[Safe rule]`
- **Oversized collections.** A collection longer than 4294967295 elements
  cannot occur with 32-bit slot indices. If one does, the encoder writes count
  `FF FF FF FF` and still writes every element; the hash is then defined but
  the stream cannot be split back into records. `[Safe rule]`
- **Out-of-range stored values.** An orientation outside 0-5, an elevation
  outside 0-2, a hex outside 0-39999, a negative quantity or a negative HP is
  written as stored (section 2.2). The hash never validates game rules.
  `[Safe rule]`
- **Combat flag without a list.** A combat state whose active flag is set but
  which holds no participants writes a present record with two zero counts.
  `[Safe rule]`

### 3.5 VM Opcode Trace (TSV)

One row per executed instruction. The file is ASCII, uses tab separators and
LF line ends, ends with an LF, and has no comment lines and no blank lines.
The first line is the header below, exactly. `[Must-match]`

```text
step	program	pc	word	imm	next_pc	vsp	rsp	fp	gp	top	status
```

| Column | Content |
| --- | --- |
| `step` | Decimal row number, from 0 |
| `program` | The program that fetched the instruction: the last component of the path it was loaded from, in lowercase, with a trailing `.hex` removed (so `tests/fixtures/vm/program.int.hex` gives `program.int`, and VFS path `scripts/Foo.INT` gives `foo.int`) |
| `pc` | Address of the instruction word, 8 uppercase hexadecimal digits |
| `word` | The fetched instruction word in full, unmasked, 4 uppercase hexadecimal digits |
| `imm` | For a constant push (dispatch slot 1, SPEC-004 section 2.5), its four-byte payload as 8 uppercase hexadecimal digits; otherwise `-` |
| `next_pc` | The instruction address after the instruction completed, 8 uppercase hexadecimal digits |
| `vsp` | Value-stack byte height after the instruction, decimal |
| `rsp` | Return-stack byte height after the instruction, decimal |
| `fp` | Local-frame base F after the instruction, signed decimal, `-1` when unset |
| `gp` | Global-area base G after the instruction, signed decimal, `-1` when unset |
| `top` | The value-stack top after the instruction as `TTTT:PPPPPPPP` (tag, then payload, uppercase hexadecimal), or `-` when the value stack is empty |
| `status` | One of `ok`, `yield`, `wait`, `blocked`, `stopped`, `ended`, `error` |

All columns are `[Must-match]`. The status is the program's state after the
instruction; when several apply, the first in this order wins: `error`,
`ended`, `stopped`, `blocked`, `wait`, `yield`, `ok`. `[Must-match]`

- `ended`: the instruction set the ended state (`0x8010`, `0x800E`).
- `stopped`: it set the stopped state (`0x8011`).
- `blocked`: it blocked the program (an attached child started by `0x8008` or
  `0x800A`, or an external call in progress).
- `wait`: it installed a wait (`0x8047`).
- `yield`: it requested a yield (`0x801D`, `0x8021`, `0x8023`, `0x8025`,
  `0x8026`).
- `error`: it raised a VM error. On an error row the columns `next_pc`,
  `vsp`, `rsp`, `fp`, `gp` and `top` are `-`, and the trace ends after that
  row. `[Must-match]`

Further rules: `[Must-match]`

- Instruction-budget boundaries (SPEC-004 section 3.3) are invisible: they
  write no row and do not reset `step`.
- An instruction executed in a host-initiated call, a condition stream or a
  child program is a row like any other, with its own `program` value, in
  execution order.
- The trace shows the engine's behavior, including any `[May-improve]`
  choices SPEC-004 lets it make. The goldens use no instruction where SPEC-004
  allows two behaviors.

### 3.6 Combat Hash Trace (TSV)

One row per combat event, same file rules as section 3.5. Header, exactly:
`[Must-match]`

```text
seq	event	round	turn	actor	combat_hash	state_hash
```

| Column | Content |
| --- | --- |
| `seq` | Decimal row number, from 0 |
| `event` | `start`, `turn` or `end` |
| `round` | The combat record's round counter at that moment, decimal; `-` on an `end` row |
| `turn` | The combat record's current turn at that moment, decimal; `-` when absent and on `start` and `end` rows |
| `actor` | On a `turn` row, the handle of the actor whose turn just ended, as `index:generation` in decimal; `-` otherwise |
| `combat_hash` | `WorldState::combat_hash()` at that moment, 16 uppercase hexadecimal digits |
| `state_hash` | `WorldState::state_hash()` at that moment, 16 uppercase hexadecimal digits |

Row moments: `[Must-match]`

- `start`: once, when combat initialization (SPEC-007 section 3.3) has
  completed and before the first turn's actor is set.
- `turn`: after every turn's end-of-turn steps (SPEC-007 section 3.3, last
  item: animation completions drained, damage this turn reset, free movement
  cleared). Turns skipped for knockout, death or lose-turn status, and the
  immediate turns of admitted waiters, each write a row.
- `end`: once, after combat cleanup has completed.

### 3.7 `asset-audit` Commands

Two dump modes are added to `asset-audit`. Each is exclusive: combining a dump
flag with another dump flag or with any `--validate-*` or `--report-*` flag is
a usage error. A dump mode prints no `Audit categories` summary. Without
`--trace-out FILE` the trace goes to stdout; with it, to FILE (created or
replaced). Diagnostics go to stderr. `[Must-match]` for the output; the flag
names are a Team A default (Decision D-23).

**VM trace.**

```sh
asset-audit --dump-vm-trace FILE [--max-steps N] [--trace-out OUT]
```

- FILE is a host path. A name ending in `.hex` is read as an offset-addressed
  hex dump in the format of `tests/fixtures/vm/program.int.hex` (lines
  `OOOO: XX XX ...`, offsets contiguous from 0); any other name is read as raw
  `.INT` bytes. `--corpus-dir` is not needed for this mode. `[Must-match]`
- The program is loaded as SPEC-004 describes and runs from offset 0 with
  empty stacks and both bases -1, under a headless host with an empty world:
  no map loaded, no objects, and the campaign GVAR array absent. The host
  clock reads 0 milliseconds throughout. The driver re-invokes the
  interpreter until the program is ended, stopped or in error. `[Must-match]`
- A host word that the headless host cannot carry out raises a VM error,
  which writes an `error` row and ends the trace. `[Safe rule]`
- `--max-steps` (default 100000) bounds the rows. Reaching it ends the trace
  without an extra row. `[Safe rule]`
- Exit status: 0 when the last row is `ended` or `stopped`; 1 for a usage or
  load error (no trace written); 2 when the trace ends on an `error` row, on
  `blocked`, `wait` or `yield` with nothing left to resume, or at the step
  limit. `[Must-match]`

**Combat trace.**

```sh
asset-audit --corpus-dir DIR --dump-combat-trace MAP [--seed S] [--max-rounds R] [--trace-out OUT]
```

`[Must-match]` unless a step is tagged otherwise:

1. Mount DIR as the other modes do, and start a new game on map MAP (a VFS
   path) under the Fallout 1 profile, with the default player sheet, the
   default language, and seed S (default 1), exactly as `vault-platform
   --seed S` seeds a new game. Run the map-entry sequence (SPEC-006 section
   3.5) to completion. No player input is given.
   **Revision 2:** the combat AI (SPEC-009 section 3.1) must be initialized
   with a nonempty packet table. If loading `combatai.msg` or `data/ai.txt`
   fails, or the table is empty, print a diagnostic naming the failure on
   stderr and exit 1 without writing a trace. `[Safe rule]` The dump never
   runs NPC turns without packets, so a golden cannot be recorded without the
   AI by mistake.
2. Choose the defender: among live entities whose PID is a critter's, that
   are not the player, are on the player's elevation and are not dead, the
   one with the smallest handle (slot index, then generation). If there is
   none, exit 1 without writing a trace.
3. Make one combat request with the player as attacker and that critter as
   defender, AP bonus 0 and no accuracy adjustment (SPEC-007 section 3.3).
   The request itself performs no attack.
4. On every player turn, end the turn at once, as the end-turn command does.
   NPC turns run the engine's combat AI (SPEC-009).
5. Advance the simulation with the same headless stepping that replays use
   (SPEC-020 section 2.3), writing each row of section 3.6 at its moment.
6. Stop after the `end` row, or once R complete rounds (default 10) have
   been traced; in the second case no `end` row is written. Exit 0.

**The AI fixture (revision 2).** The command reads the corpus as it is and
installs nothing itself. Every run that produces or checks a seeded combat
golden (test T11, and the recording of section 3.9) first prepares the corpus
directory it traces, after `cargo xtask fixtures` has written it: `[Must-match]`

| Copy from `tests/fixtures/spec021/` | To, under the traced corpus directory | Since |
| --- | --- | --- |
| `rev2/ai.txt` | `data/ai.txt` | Revision 2 |
| `rev2/combatai.msg` | `text/english/game/combatai.msg` | Revision 2 |
| `rev2/sampleaq.frm` | `art/critters/sampleaq.frm` | Revision 3 |
| `rev2/sampleao.frm` | `art/critters/sampleao.frm` | Revision 3 |
| `rev2/sampleba.frm` | `art/critters/sampleba.frm` | Revision 3 |
| `rev2/samplecj.frm` | `art/critters/samplecj.frm` | Revision 3 |

Each copy is a plain byte-for-byte file copy. It replaces any file already at
that path; directories are created as needed. The destination names are
lowercase, like the corpus's own `art/critters/sampleaa.frm`. `english` is
the default language (SPEC-014 section 3.1), which step 1 uses. The copies go
into the test's own corpus directory, never into the committed fixture tree.
`cargo xtask fixtures` itself is not changed by revisions 2 or 3, so every
other test sees the corpus as before. Section 5.7 explains why this packet
makes the NPC fight, and section 5.8 why its attack needs the four art files
(revision 3).

### 3.8 Golden Files and Re-Recording

Goldens and vectors for this spec live in `tests/fixtures/spec021/`, never in
`oracle/`, which `PROVENANCE.md` reserves for research-side vectors.
`[Must-match]`

- The vectors and VM traces of section 5 were computed by Team A from this
  spec alone and are committed with it. They are fixed: an implementation
  that disagrees is wrong, unless a spec revision changes them.
- The **seeded combat golden** cannot be computed by hand, because it depends
  on the combat AI, hit rolls and animation stepping. The implementer records
  it once (section 4, test T11), reviews it and commits it as
  `tests/fixtures/spec021/combat-trace-synthetic-seed1.tsv`. From then on it
  is `[Must-match]`.
- A later change may re-record the seeded combat golden only under SPEC-020
  section 3.4's rule: the commit names the file and the approved rule that
  justifies the change. A change to the stream's coverage needs a new magic
  (section 2.3) and a spec revision.

### 3.9 Revision 2 and 3 Files, Supersession, and the Implementer's Follow-Up

Revision 2 adds the folder `tests/fixtures/spec021/rev2/`. The revision 1
files stay where they are until the implementer adopts revision 2, because the
merged tests read them. Team A did not edit them. `[Must-match]`

| File | Status in revision 3 |
| --- | --- |
| `fnv1a64.tsv` | Current, unchanged |
| `w1.combat.hex`, `w4.combat.hex` | Current, unchanged: the item entity is not a combat participant, so the combat streams and every `combat_hash` stay the same |
| `vm-trace-program.tsv`, `vm-trace-program-branch.tsv` | Current, unchanged |
| `w0.stream.hex` to `w4.stream.hex` | **Superseded** by `rev2/w0.stream.hex` to `rev2/w4.stream.hex` |
| `hashes.tsv` | **Superseded** by `rev2/hashes.tsv` |
| `combat-trace-example.tsv` | **Superseded** by `rev2/combat-trace-example.tsv` (only the `state_hash` column differs) |
| `combat-trace-synthetic-seed1.tsv` | **To be re-recorded** with the AI fixture and the art fixture installed (below) |
| `rev2/ai.txt`, `rev2/combatai.msg` | New in revision 2: the AI fixture of sections 3.7 and 5.7 |
| `rev2/sampleaq.frm`, `rev2/sampleao.frm`, `rev2/sampleba.frm`, `rev2/samplecj.frm` | New in revision 3: the art fixture of sections 3.7 and 5.8 |
| `rev2/README.md` | New in revision 2: describes the folder. Revision 3 adds the four art files to it |

Revision 3 puts its files in `rev2/` too, so that everything T11 installs
comes from one folder (Decision D-38). It changes no revision 2 file other
than `rev2/README.md`.

The superseded files hold revision 1's W0, whose item has no entity. They
become superseded when the implementer's follow-up commit adopts revision 2.
From then on no test reads them, and that commit deletes them. Until then they
are still the merged tests' inputs and must not be edited.

**What the implementer changes.** One follow-up commit by the implementer
(not Team A) does all of this: `[Must-match]`

1. **T2, T3, T4, T6 and T5's direct check** use the revision 2 W0 to W4 of
   section 5 and read `rev2/w0.stream.hex` to `rev2/w4.stream.hex` and
   `rev2/hashes.tsv`. They keep reading `w1.combat.hex`, `w4.combat.hex` and
   `fnv1a64.tsv` from the top folder. T4's flip counts become 1800 for W0 and
   1864 for W1. T6's RNG record moves to stream offsets `0x7D` to `0x104`.
   T5 checks W0's hash directly with handles `1:1`, `2:3` and the item `3:1`.
2. **T10** reads `rev2/combat-trace-example.tsv`.
3. **The encoder** orders entity records with equal handles by the rest of
   their bytes (sections 2.4 and 3.2). No real world and no section 5 vector
   has equal handles. T4 builds two flips that do (bit 1 of the index of
   `1:1` gives `3:1`, and bit 1 of the index of `3:1` gives `1:1`). Team A
   checked that every T4 flip is distinct with and without the rule.
4. **The combat trace dump** refuses to run without a loaded, nonempty packet
   table (section 3.7, step 1).
5. **T11** installs the AI fixture and, since revision 3, the art fixture in
   its corpus directory: all six rows of section 3.7's table. It asserts that
   the NPC acts (section 4, T11).
6. **The seeded golden** `tests/fixtures/spec021/combat-trace-synthetic-seed1.tsv`
   is re-recorded at the same path, with the AI fixture and the art fixture
   installed, by the T11 run, and reviewed. This is a re-recording under
   SPEC-020 section 3.4's rule: the commit names the file and cites this
   section and section 3.7 of SPEC-021 revision 3 as the approved rule that
   justifies it. The line in `tests/fixtures/spec021/README.md` that
   describes the golden is updated to say that the AI fixture and the art
   fixture were installed, and the sentence saying the NPC's turns run without
   AI actions is removed.
7. The superseded files of the table above are deleted, and
   `tests/fixtures/spec021/README.md` points to `rev2/`.

**Revision 3: where the follow-up stopped, and what changes.** The
implementer began this follow-up in Wave 4 and stopped at step 5, as T11's
stop rule requires (section 4): with the AI fixture installed, the NPC still
spent no AP. Section 5.8 records the measured finding. Revision 3 changes
only steps 5 and 6 above: T11 also copies the four art files, and the golden
is recorded with them. Steps 1 to 4 and 7 are unchanged; work already done
for them stands. The golden must not be committed from a run that lacks the
art fixture, because a run without it records an NPC that never acts.
`[Must-match]`

### 3.10 Replay Checkpoints (Revision 4)

A **checkpoint** is one row that records a run's covered state at the end of
one step. The rows of a run are its **checkpoint stream**. Two runs with the
same inputs give the same stream, so a stream shows not only that a replay
diverged, but at which step.

**The headless step.** Every entry point runs the same steps in the same order
through `vault_platform::session`'s `SessionRuntime` (SPEC-020 section 2.3).
One **headless step** is one frame of that pipeline: `[Must-match]`

| Order | Phase | What happens |
| ---: | --- | --- |
| 1 | Input | The step takes its input records, in log order |
| 2 | Simulation | The world advances by the frame's clock: world ticks, scripts, animation, combat turns, and modal state |
| 3 | **Checkpoint** | The row is written, here and nowhere else |
| 4 | Presentation | The refresh is drawn on either render path (SPEC-022), and the frame or digest is taken |

The records a step takes: `[Must-match]`

- In a log with a leading `surface` record, a step takes every record up to and
  including the next `tick`.
- In a log without one, each command is one step, a simulated 16 ms frame
  (`docs/REFERENCE.md`).
- In a driver run (section 3.7), a step takes the driver's commands since the
  last step: the combat request, and each end of a player turn.

**The rules of a checkpoint.** `[Must-match]`

1. **One row per step.** Every step writes exactly one row. A step under a
   modal screen that suspends the world still writes its row, and a step that
   runs more than one world tick (a `game_speed` above 1.0) still writes one.
2. **The first row.** Row 0 is written once, when the session has been built
   and before the first step. It has `tick` 0 and `input_index` 0. So a run of
   N steps has N + 1 rows.
3. **The moment.** A row is written after the last simulation change of its
   step and before anything of that step is presented. Rendering draws from the
   state the row describes. Because drawing never changes the world (SPEC-022
   section 3.13, rule 1), the stream does not depend on the render path. If a
   refresh ever did change a covered value, the next row would show it.
4. **Reading only.** Taking a checkpoint reads the world (section 3.3). It
   draws no random number, allocates no handle and changes no state.
5. **What writes no row.** A record that opens no step writes no row: `quit`,
   and any record after the last `tick`. A replay that stops at an unconsumed
   Escape, or a driver run that stops at its round limit, writes no row for
   that unfinished step.
6. **Off by default.** Without a checkpoint flag no row is computed, and with
   one the replay's `state_sha256`, its frames and every input-log and replay
   record are the same as without it.
7. **The session's own order.** If the session applies a frame's records at
   another point of the frame than phase 1, the row's moment does not move: it
   stays the last thing before presentation, and `input_index` counts the
   records applied by then. The implementer records the actual order in the
   commit. `[Safe rule]`

**The stream format.** The file rules of section 3.5 apply: ASCII, tab
separators, LF line ends, a final LF, no comment lines and no blank lines.
Line 1 is the version line and line 2 the column header, exactly:
`[Must-match]`

```text
vault-checkpoints	1	VSH1	VCH1
tick	input_index	state_hash	combat_hash
```

| Line 1 field | Content |
| --- | --- |
| 1 | The format's name, `vault-checkpoints` |
| 2 | The format version, decimal, now 1 |
| 3 | The world stream's magic (section 2.3) |
| 4 | The combat stream's magic (section 2.7) |

| Column | Content |
| --- | --- |
| `tick` | The step number, decimal, 0 for the first row, then rising by exactly 1. It counts headless steps: it is not a clock, and not a world tick |
| `input_index` | The zero-based index, in the log, of the next record not yet applied, decimal. This is also the number of records applied so far, every line of the log included, and it never falls. In a driver run it counts the driver's commands |
| `state_hash` | `WorldState::state_hash()`, 16 uppercase hexadecimal digits |
| `combat_hash` | `WorldState::combat_hash()`, 16 uppercase hexadecimal digits, while a combat is active (section 2.6); otherwise `-`. The constant of section 2.7 is never written |

Fields 3 and 4 of line 1 are taken from the encoder's own magics, so a change
of coverage changes line 1 by itself. Any change to the columns, to the moment
of a row or to the rules of this section raises the format version.
`[Must-match]`

**The comparison rule.** A stream is compared with an expected stream line by
line, in file order, and the comparison **stops at the first failure and
reports that one line**. It never goes on to count later differences, because
every row after a divergence is expected to differ. `[Must-match]`

1. Lines 1 and 2 are compared as bytes. A difference is a format mismatch, and
   no row is compared: streams of two formats, or of two stream magics, cannot
   be compared.
2. Then, for each row line in turn:
   - if the actual stream has ended, it **ended early**;
   - if the actual line is not a well-formed row, it is **malformed**. A
     well-formed row has four columns, decimal numbers with no leading zero and
     no sign, hashes of 16 uppercase hexadecimal digits (`-` allowed for
     `combat_hash`), a `tick` one above the last row's (0 on the first row), and
     an `input_index` not below the last row's (0 on the first row);
   - if the expected stream has ended, the actual stream is **too long**;
   - otherwise the columns are compared in order, and the first that differs is
     a **mismatch**.

The report is one line, with these exact texts (a tab in a quoted line is
shown as a space): `[Must-match]`

| Verdict | Report |
| --- | --- |
| Equal | `checkpoints match: N rows` |
| Format | `checkpoint format mismatch: line L expected E, actual A` |
| Mismatch | `checkpoint mismatch at tick T: COLUMN expected E, actual A` |
| Ended early | `checkpoint stream ended early: expected tick T, actual ended after tick U` |
| Too long | `checkpoint stream too long: unexpected tick T` |
| Malformed | `checkpoint row malformed: line L` |

**Commands.** The flag names are a Team A default (D-45).

- `vault-platform --replay-inputs LOG --checkpoints-out FILE` writes the stream
  (created or replaced). `--checkpoints-expect FILE` compares each row as it is
  produced. The two may be combined, and both work with every option of a
  replay.
- `asset-audit --dump-combat-trace MAP --checkpoints-out FILE` and
  `--checkpoints-expect FILE` do the same for a driver run. The trace of section
  3.6 stays as it is.
- With `--checkpoints-expect`, the run **stops at the step of the first
  failure**, prints the report on stderr, prints no digest line, and exits with
  status 2. With a stream that matches to the end, the run exits as it would
  without the flag. A usage error or an unreadable expected file exits with
  status 1 before the run starts. If the binary already gives status 2 another
  meaning, the implementer picks a free status and records it. `[Must-match]`

**The lockstep unit.** This stream is the synchronization unit for future
lockstep multiplayer (SPEC-025, not yet written). `[Must-match]` for the format;
the rest of this paragraph binds SPEC-025, not this revision.

- A peer's stream of one session is exactly this format: the same version line,
  the same columns, one row per step. `tick` is the step both peers agree on,
  and `input_index` lets a peer see that the other has applied the same inputs.
- The version line is the handshake: two peers whose lines differ cannot
  compare hashes, and must not start.
- A row is at most 56 bytes with its LF, for a `tick` and an `input_index` of up
  to 10 digits, so a peer can send every row.
- SPEC-025 defines the transport and what a peer does on a mismatch. It must
  also deal with two limits of this stream. The hash covers only section 3.1's
  values, so a difference in an uncovered value (the game clock, the timed
  queue, world-map travel) shows only once it reaches a covered one; closing
  that gap means a new magic (section 2.3). And FNV-1a is not collision-resistant
  against a chosen input (section 2.1), so the stream detects divergence between
  honest peers, and is no defense against a cheating one.

### 3.11 The Three Replay Scenarios (Revision 4)

Three tests run the checkpoint stream over existing synthetic fixtures. They
add no fixture to the shared corpus, and `cargo xtask fixtures` is unchanged.
`[Must-match]`

| | (a) Exploration and dialogue | (b) World-map travel | (c) Seeded multi-turn combat |
| --- | --- | --- | --- |
| Test | T16 | T17 | T18 |
| Corpus | `cargo xtask fixtures --output DIR` | The same | The same, prepared as T11 prepares it: all six rows of section 3.7's table |
| Map | `maps/synthetic.map` | `maps/synthetic.map` | `maps/synthetic.map` |
| Seed | 1 | 1 | 1 |
| Input | `rev3/scenario-explore-talk.jsonl` | `rev3/scenario-world-travel.jsonl` | The driver of section 3.7, `--max-rounds 5` |
| Steps | 387 | 615 | Whatever the run takes |
| Golden | `rev3/scenario-explore-talk.checkpoints.tsv` | `rev3/scenario-world-travel.checkpoints.tsv` | `rev3/scenario-seeded-combat.checkpoints.tsv` |

Scenarios (a) and (b) run as `vault-platform --corpus-dir DIR --preview-map
maps/synthetic.map --seed 1 --replay-inputs LOG`, on the default render path.
Section 5.10 gives the phases of each log.

**Goldens.** No golden here can be computed by hand, because each depends on
routing, scripts, dice and animation stepping. The implementer records each one
once, reviews it against the assertions below, and commits it. From then on it
is `[Must-match]`, and it may be re-recorded only under section 3.8's rule.

**Assertions.** Each test checks these on the live session while it records,
and again whenever it replays. A digest of a row cannot show what the session
was doing, so the assertions read the session itself. `[Must-match]`

- **(a).** The player's hex at step 133 differs from step 10, and `state_hash`
  differs between those rows. The Guide's conversation is open at step 261 and
  closed at step 387. Every `combat_hash` is `-`.
- **(b).** The world map is open at step 12 and still open at step 615: arrival
  never enters a town (`docs/REFERENCE.md`). The party's position at step 615
  differs from (825, 75), and travel has finished. The loaded map has not
  changed, so no encounter happened. `state_hash` differs between at least two
  rows from step 12 to step 615: a trip of 160 pixel steps passes at least one
  W-iteration check, because W is at most 120, and each check draws three d6
  (SPEC-008 sections 3.4 and 3.5). Every `combat_hash` is `-`.
- **(b), uncovered state.** World-map travel state is not covered by the hash
  (section 3.1), so the stream alone sees travel only through those draws. T17
  therefore also writes the party's final position and the final game tick to
  `rev3/scenario-world-travel.final.tsv`, with the header `x`, `y`, `game_tick`,
  and compares that row too. The implementer records it with the golden.
- **(c).** Rows with a `combat_hash` exist, and row 0 has none. The NPC acts, by
  T11's own assertion. The last row's `input_index` is 1 + the number of player
  `turn` rows of the trace: the request, and one end of turn for each. The
  trace of section 3.6 from the same run still equals
  `combat-trace-synthetic-seed1.tsv` byte for byte, so recording checkpoints
  changes nothing. A second run with seed 1 gives the same stream, and a run
  with seed 2 differs in at least one row.

**Stop rule.** If an assertion fails while recording, the implementer stops and
asks for a spec revision, as T11's stop rule says, and does not change a log or
invent a scenario. The report names the assertion, the step, and what the
session was doing. In particular, if an encounter fires in (b), the synthetic
corpus has no encounter maps (a longer trip at seed 1 asked for
`maps/mountn1.map`, measured by Team A), so the trip must be replaced by a
revision. `[Must-match]`

**Expected diagnostics.** The synthetic corpus logs two warnings in these runs,
and neither is a failure: `combat AI unavailable` in (a) and (b), whose corpus
has no `combatai.msg`; and, in (b), a world diagnostic about water global 10,
because the synthetic campaign has fewer globals (measured by Team A). Every
replay on this corpus also logs `replay digest without a central save`, so its
`state_sha256` covers the frame and not the central save (measured by Team A).
That makes the checkpoint stream the stronger oracle on the synthetic corpus.

### 3.12 Revision 4 Files and the Implementer's Follow-Up

Revision 4 adds the folder `tests/fixtures/spec021/rev3/`. It changes no file of
an earlier revision, and supersedes none. `[Must-match]`

| File | Status in revision 4 |
| --- | --- |
| `rev3/checkpoints-example.tsv` | New: the format example of section 5.9 |
| `rev3/compare-cases.tsv` | New: the comparison cases of section 5.9 |
| `rev3/scenario-explore-talk.jsonl`, `rev3/scenario-world-travel.jsonl` | New: the logs of scenarios (a) and (b) |
| `rev3/README.md` | New: describes the folder |
| `rev3/scenario-explore-talk.checkpoints.tsv`, `rev3/scenario-world-travel.checkpoints.tsv`, `rev3/scenario-seeded-combat.checkpoints.tsv`, `rev3/scenario-world-travel.final.tsv` | To be recorded by the implementer (section 3.11) |
| Every file of `tests/fixtures/spec021/` and `rev2/` | Current and unchanged |

**What the implementer changes.** One follow-up commit by the implementer (not
Team A) does all of this: `[Must-match]`

1. **The format.** Add the row writer, the reader and the comparison beside the
   hash, so that every tool shares one implementation. The module is free, and
   it holds no platform dependency.
2. **The step.** Call the writer in `vault_platform::session`'s headless step at
   the moment of section 3.10, and write row 0 once.
3. **The driver.** Have the combat trace of section 3.7 write rows at the same
   moment, with `input_index` counting its commands.
4. **The commands.** Add the flags of section 3.10.
5. **The tests.** Add T13 to T15 and T19, which need no golden.
6. **The goldens.** Record the three streams and the final row of (b), review
   them against section 3.11's assertions, and commit them with T16 to T18. Add
   a line to `rev3/README.md` for each golden.
7. **The documents.** Describe the flags and the stream in `docs/REFERENCE.md`.

No existing golden is re-recorded. The magics, T1 to T12 and the vectors of
sections 5.1 to 5.8 stay as they are.

## 4. Required Tests

The implementer writes at least these tests. They use synthetic data only.
`[Must-match]`

- **T1. FNV-1a known answers.** `fnv1a64` reproduces every row of
  `fnv1a64.tsv`, and the step table of section 5.1.
- **T2. Encoder vectors.** Build the snapshots W0 to W4 of section 5 from
  plain data. Their `canonical_bytes` equal `rev2/w0.stream.hex` to
  `rev2/w4.stream.hex` byte for byte; `canonical_combat_bytes` of W1 and W4
  equal `w1.combat.hex` and `w4.combat.hex`; every hash equals its row in
  `rev2/hashes.tsv`, including the mutation rows.
- **T3. Order independence.** W2 is supplied with its entities and LVAR
  slices out of order and still encodes as `rev2/w2.stream.hex`. Also fill the
  LVAR slices (and any other keyed collection the snapshot builder reads from
  a map type) in at least two different insertion orders: the hash is the
  same.
- **T4. Single-bit sensitivity of the encoder.** For W0 and W1, flip each of
  the 32 bits of every fixed-width covered scalar in turn, one flip at a time:
  each handle index and generation, PID bits 0-23 (bits 24-31 change the
  critter rule and so the stream length), elevation, hex, orientation, HP,
  AP, each inventory PID and quantity, the RNG state, selector and all 32
  entries, the GVAR value, and in W1 the round counter and current turn. Each
  flip gives a state hash different from the base and from every other flip
  (Team A checked all 1800 flips of W0 and 1864 of W1; revision 1's W0,
  without the item entity, had 1680 and 1744). The item entity `3:1` adds its
  handle index and generation, PID bits 0-23 and orientation. Two of the
  flips give two records the same handle, which section 2.4 orders. Each flip
  of a combat field, and of a participant's HP, AP, elevation, hex or
  orientation, also changes the combat hash. Also toggle each opt field once
  (present to absent, or the reverse) and check that the hash changes.
- **T5. Extraction.** Build a `WorldState` through the engine's ordinary API
  with two critters (one holding a stack of an item), one GVAR, map
  variables, and two scripts with local variables. `state_snapshot` returns
  exactly the values set, with the handles the store actually allocated, and
  `state_hash` equals the hash of the snapshot built by hand from those
  values. The held stack appears as the holder's inventory entry and as its
  own entity record with no position (section 2.4, "Held items"). Where the
  allocator produces the critters as `1:1` and `2:3` and the item as `3:1`,
  with the item's orientation 0, also check W0's hash directly.
- **T6. RNG extraction.** A generator with a zeroed shuffle array, seeded
  with 1 by SPEC-006 section 3.4 and before any draw, snapshots to the RNG
  record of W0 (stream offsets `0x7D` to `0x104`; revision 1's W0 had it at
  `0x66` to `0xED`); after one draw, to W3's.
- **T7. Seeded determinism.** Build two worlds from the same synthetic corpus
  with the same seed and drive both through the combat trace of section 3.7
  (in-process, not through the binary) for at least 5 rounds or until combat
  ends: every row is equal, and in particular the `state_hash` after every
  turn N. Repeat with ordinary world stepping for at least 200 ticks outside
  combat, comparing `state_hash` after every tick. A third world with a
  different seed differs in at least one row, if its trace makes any random
  draw.
- **T8. Single-bit sensitivity of the world.** On a live `WorldState`, change
  each of these by one bit through the engine's API and check that
  `state_hash` changes: a GVAR, an MVAR, an LVAR element, a critter's HP, its
  AP, its hex, its orientation, an inventory quantity, and one RNG draw. Then
  change values that are not covered (advance the game clock, change the
  camera) and check that `state_hash` does not change.
- **T9. VM trace goldens.** Tracing `tests/fixtures/vm/program.int.hex` gives
  `vm-trace-program.tsv` byte for byte. Tracing it with the four bytes at
  `0x00AE` replaced by `00 00 00 01` (SPEC-004 section 4.1's branch mutation)
  gives `vm-trace-program-branch.tsv`. At least one test runs the
  `asset-audit` binary on the `.hex` fixture and checks exit status 0.
- **T10. Combat trace format.** Writing rows for the snapshots S0 to S3 of
  section 5.6 gives `rev2/combat-trace-example.tsv` byte for byte, and a reader of
  the format reads it back to the same rows.
- **T11. Seeded combat golden.** Generate the synthetic corpus with `cargo
  xtask fixtures --output DIR`, install the AI fixture and the art fixture of
  section 3.7 (all six rows of its table) in the directory that holds the
  map, and run the section 3.7 combat trace with
  seed 1 and `--max-rounds 5` on the first of these maps that has a defender:
  `maps/synthetic.map` in DIR, then `maps/cam_demo.map` in `DIR-cam-demo`.
  The output equals `combat-trace-synthetic-seed1.tsv`, which the
  implementer records on the first passing run, together with a line in
  `tests/fixtures/spec021/README.md` naming the map used. Revision 2: the
  same test, run in process, also asserts that the NPC acts. At the end of at
  least one of the defender's traced turns, its current AP is below its
  effective maximum AP, because it moved or attacked (SPEC-009 section 3.12).
  If neither map has a defender, the trace has no NPC `turn` row, or the NPC
  spends no AP in any traced turn with the AI fixture and the art fixture
  installed, the implementer stops and asks for a spec revision instead of
  inventing a scenario. In the last case the report names the SPEC-009 step
  that ended the NPC's turn without an action, and, when a request or an
  animation sequence was refused, the SPEC-007 rule and the missing resource
  (as the Wave 4 report did, section 5.8). Revision 3: the art fixture names
  the art base `sample`, the only row of the corpus's critter list. If the
  traced defender's art identifier names another base, or the list is no
  longer the one-row `sample,0`, the implementer stops in the same way.
- **T12. No unstable hashing.** The module that builds the stream uses no
  `std::hash` item, no `HashMap` or `HashSet` iteration, and no `usize` in
  the stream. Enforcing this with clippy's `disallowed_types` and
  `disallowed_methods` for that module is recommended. `[May-improve]`
- **T13. Checkpoint format (revision 4).** Writing the rows of section 5.9
  gives `rev3/checkpoints-example.tsv` byte for byte, and a reader of the format
  reads it back to the same rows. The version line's fields 3 and 4 come from
  the encoder's magics, and a row's `combat_hash` is `-` exactly when no combat
  is active.
- **T14. Comparison rule (revision 4).** Applying each row of
  `rev3/compare-cases.tsv` to a copy of the example, and comparing that copy
  against the example, gives the row's verdict and its exact report. The
  comparison stops at the first failure: in case `first-of-two` the report
  names only tick 2.
- **T15. The step (revision 4).** In a replay of a synthetic log:
  - N steps give N + 1 rows, with `tick` from 0 to N, and `input_index` ending
    at the number of records applied;
  - a world change made in the simulation phase of step S appears in row S, not
    in row S + 1;
  - with a hook on the presentation phase, the world's `state_hash` at the start
    of presentation equals the row just written;
  - the run's `state_sha256` is the same with and without `--checkpoints-out`;
  - `--checkpoints-expect` with a golden holding one changed row stops at that
    step, prints the report, prints no digest line, and exits with status 2.
    The run makes no step past that row.
- **T16. Scenario (a) (revision 4).** Run section 3.11's scenario (a). The
  stream equals its golden, and section 3.11's assertions hold.
- **T17. Scenario (b) (revision 4).** Run scenario (b). The stream equals its
  golden, the final row equals `rev3/scenario-world-travel.final.tsv`, and the
  assertions hold.
- **T18. Scenario (c) (revision 4).** Run scenario (c). The stream equals its
  golden, and the assertions hold, including the second run and the other seed.
- **T19. Render path (revision 4).** Replaying scenario (a) on
  `--render-path reference` and `instanced` gives the same stream. Without a GPU
  adapter the second run prints `skipped: no GPU adapter` and passes, unless
  `VAULT_REQUIRE_GPU` is `1` (SPEC-022 section 4). A difference fails the test,
  and is a SPEC-022 section 3.13 defect, not a re-recording.

## 5. Synthetic Test Fixtures (Hex / Text)

All values here are synthetic: the PIDs, handles, script identifiers and
variable values were chosen for this spec. The RNG values follow from
SPEC-006 section 3.4 with seed 1, and the VM traces from SPEC-004 section
4.1's synthetic program. Files are in `tests/fixtures/spec021/`; the vectors
that revision 2 changed or added, and revision 3's art fixture, are in its
`rev2/` folder (section 3.9).

### 5.1 FNV-1a Step Table

The combat stream with no combat active, `VCH1`, `CMBT`, `00`, hashed one
byte at a time. Each row gives the byte, H after the XOR, and H after the
multiplication.

| Byte | After XOR | After multiply |
| --- | --- | --- |
| (start) | | `CBF29CE484222325` |
| `56` | `CBF29CE484222373` | `AF640B4C86023C69` |
| `43` | `AF640B4C86023C2A` | `093B5D07B5CC3B5E` |
| `48` | `093B5D07B5CC3B16` | `7C1A2A19EA086662` |
| `31` | `7C1A2A19EA086653` | `E8DBDD08AC45DF09` |
| `43` | `E8DBDD08AC45DF4A` | `F377DFBCBABA6ABE` |
| `4D` | `F377DFBCBABA6AF3` | `6F1C20B14AC3BAE9` |
| `42` | `6F1C20B14AC3BAAB` | `908638420A963091` |
| `54` | `908638420A9630C5` | `2A425D37FD34DEBF` |
| `00` | `2A425D37FD34DEBF` | `03A3252340D67E8D` |

So `combat_hash` with no combat active is `03A3252340D67E8D`.

### 5.2 The Tiny World W0 (Revision 2)

| Item | Value |
| --- | --- |
| Entity `1:1` | PID `0x01000001` (a critter); elevation 0, hex 20100; orientation 2; HP 30; AP 8; inventory: one entry, item PID `0x00000004`, quantity 3 |
| Entity `2:3` | PID `0x01000002` (a critter); elevation 0, hex 20102; orientation 5; HP 12; AP 6; empty inventory |
| Entity `3:1` | PID `0x00000004` (an item, type 0): the stack of 3 that `1:1` holds; no position; orientation 0; no HP or AP; empty inventory |
| RNG | A zeroed generator seeded with 1 (SPEC-006 section 3.4), before any draw: state 784575628, selector 0, entries below |
| Combat | None |
| GVAR | One variable: [7] |
| MVAR | Absent (no map loaded) |
| LVAR | No slices |

Revision 1's W0 had no entity `3:1`: its item existed only as `1:1`'s
inventory entry, which the engine cannot represent (open item 4). Revision 2
adds the item as its own entity, in the form section 2.4 ("Held items")
describes. The handle `3:1` is the one the engine's allocator gives the item
after the two critters, so T5 can build this world through the API.

RNG entries 0 to 31, decimal: 0, 784575628, 2128236580, 1624379149,
1358580979, 704877633, 1580723810, 101929267, 563613512, 1636807826,
1954899097, 1505795335, 893351816, 1404280278, 197493099, 1131570933,
1817129560, 1998097157, 1264817709, 1474833169, 896544303, 143542612,
823378840, 16531729, 1441282327, 1137522503, 114807987, 74243042,
1784484492, 1115438165, 823564440, 2007237709. Drawing from this state gives
SPEC-006 section 4.4's raw outputs 0, 812987216, 1817129560, ..., which checks
the values.

The complete 292-byte stream (`rev2/w0.stream.hex`):

```text
0000: 56 53 48 31 45 4E 54 53 03 00 00 00 01 00 00 00
0010: 01 00 00 00 01 00 00 01 01 00 00 00 00 84 4E 00
0020: 00 02 00 00 00 01 1E 00 00 00 01 08 00 00 00 01
0030: 00 00 00 04 00 00 00 03 00 00 00 02 00 00 00 03
0040: 00 00 00 02 00 00 01 01 00 00 00 00 86 4E 00 00
0050: 05 00 00 00 01 0C 00 00 00 01 06 00 00 00 00 00
0060: 00 00 03 00 00 00 01 00 00 00 04 00 00 00 00 00
0070: 00 00 00 00 00 00 00 00 00 52 4E 47 53 8C AC C3
0080: 2E 00 00 00 00 00 00 00 00 8C AC C3 2E 24 50 DA
0090: 7E 0D 0F D2 60 F3 4C FA 50 41 94 03 2A 62 EE 37
00A0: 5E 33 51 13 06 48 0F 98 21 92 B4 8F 61 99 64 85
00B0: 74 07 9D C0 59 88 77 3F 35 D6 9D B3 53 6B 81 C5
00C0: 0B F5 66 72 43 58 32 4F 6C 05 8B 18 77 2D 96 63
00D0: 4B 11 2B E8 57 2F 2E 70 35 54 49 8E 08 98 C3 13
00E0: 31 11 41 FC 00 17 39 E8 55 47 37 CD 43 B3 D4 D7
00F0: 06 E2 DB 6C 04 8C 12 5D 6A 55 3C 7C 42 98 98 16
0100: 31 4D 04 A4 77 43 4D 42 54 00 47 56 41 52 01 01
0110: 00 00 00 07 00 00 00 4D 56 41 52 00 4C 56 41 52
0120: 00 00 00 00
```

Field by field:

| Offset | Bytes | Meaning |
| --- | --- | --- |
| `0x000` | `56 53 48 31` | Magic `VSH1` |
| `0x004` | `45 4E 54 53` | Tag `ENTS` |
| `0x008` | `03 00 00 00` | Three entity records |
| `0x00C` | `01 00 00 00` `01 00 00 00` | Handle `1:1` |
| `0x014` | `01 00 00 01` | PID `0x01000001` |
| `0x018` | `01` `00 00 00 00` `84 4E 00 00` | Position present: elevation 0, hex 20100 (`0x4E84`) |
| `0x021` | `02 00 00 00` | Orientation 2 |
| `0x025` | `01` `1E 00 00 00` | HP present: 30 |
| `0x02A` | `01` `08 00 00 00` | AP present: 8 |
| `0x02F` | `01 00 00 00` | One inventory entry |
| `0x033` | `04 00 00 00` `03 00 00 00` | Item PID 4, quantity 3 |
| `0x03B` | `02 00 00 00` `03 00 00 00` | Handle `2:3` |
| `0x043` | `02 00 00 01` | PID `0x01000002` |
| `0x047` | `01` `00 00 00 00` `86 4E 00 00` | Position present: elevation 0, hex 20102 (`0x4E86`) |
| `0x050` | `05 00 00 00` | Orientation 5 |
| `0x054` | `01` `0C 00 00 00` | HP present: 12 |
| `0x059` | `01` `06 00 00 00` | AP present: 6 |
| `0x05E` | `00 00 00 00` | Empty inventory |
| `0x062` | `03 00 00 00` `01 00 00 00` | Handle `3:1` (the held item) |
| `0x06A` | `04 00 00 00` | PID `0x00000004` (type 0, an item) |
| `0x06E` | `00` | Position absent: held in an inventory |
| `0x06F` | `00 00 00 00` | Orientation 0 |
| `0x073` | `00` | HP absent (not a critter) |
| `0x074` | `00` | AP absent (not a critter) |
| `0x075` | `00 00 00 00` | Empty inventory |
| `0x079` | `52 4E 47 53` | Tag `RNGS` |
| `0x07D` | `8C AC C3 2E` | State 784575628 (`0x2EC3AC8C`) |
| `0x081` | `00 00 00 00` | Selector 0 |
| `0x085` | 128 bytes | Entries 0 to 31; entry 1 at `0x089` is `8C AC C3 2E`, entry 31 at `0x101` is `4D 04 A4 77` (2007237709) |
| `0x105` | `43 4D 42 54` | Tag `CMBT` |
| `0x109` | `00` | No combat |
| `0x10A` | `47 56 41 52` | Tag `GVAR` |
| `0x10E` | `01` `01 00 00 00` `07 00 00 00` | GVAR present: one element, 7 |
| `0x117` | `4D 56 41 52` | Tag `MVAR` |
| `0x11B` | `00` | MVAR absent |
| `0x11C` | `4C 56 41 52` | Tag `LVAR` |
| `0x120` | `00 00 00 00` | No LVAR slices |

The stream is revision 1's 269-byte W0 with the entity count at `0x008`
raised from 2 to 3 and the 23-byte record of `3:1` inserted at `0x062`.

**`state_hash(W0) = 0258CFED4EA8A13E`.** `combat_hash(W0) = 03A3252340D67E8D`.
Revision 1's W0 hash, `FB51804ECBBC1AB5`, is superseded (section 3.9).

### 5.3 W1: W0 in Combat

W1 is W0 with a combat record: round 0, current turn 0, active list
[`1:1`, `2:3`], empty waiting list. Its 369-byte stream
(`rev2/w1.stream.hex`) equals W0's up to `0x108`; then `0x109` holds `01` and
the 77 further bytes of the combat record, and W0's GVAR, MVAR and LVAR parts
follow from `0x157`.

The item `3:1` is not a participant, so the combat stream is the same as in
revision 1. The 86-byte combat stream (`w1.combat.hex`, unchanged):

```text
0000: 56 43 48 31 43 4D 42 54 01 00 00 00 00 01 00 00
0010: 00 00 02 00 00 00 01 00 00 00 01 00 00 00 01 1E
0020: 00 00 00 08 00 00 00 01 00 00 00 00 84 4E 00 00
0030: 02 00 00 00 02 00 00 00 03 00 00 00 01 0C 00 00
0040: 00 06 00 00 00 01 00 00 00 00 86 4E 00 00 05 00
0050: 00 00 00 00 00 00
```

| Offset | Bytes | Meaning |
| --- | --- | --- |
| `0x00` | `56 43 48 31` `43 4D 42 54` | Magic `VCH1`, tag `CMBT` |
| `0x08` | `01` | Combat present |
| `0x09` | `00 00 00 00` | Round 0 |
| `0x0D` | `01` `00 00 00 00` | Current turn present: 0 |
| `0x12` | `02 00 00 00` | Two active participants |
| `0x16` | `01 00 00 00` `01 00 00 00` `01` | Handle `1:1`, live |
| `0x1F` | `1E 00 00 00` `08 00 00 00` | HP 30, AP 8 |
| `0x27` | `01` `00 00 00 00` `84 4E 00 00` `02 00 00 00` | Position present: 0, 20100; orientation 2 |
| `0x34` | `02 00 00 00` `03 00 00 00` `01` | Handle `2:3`, live |
| `0x3D` | `0C 00 00 00` `06 00 00 00` | HP 12, AP 6 |
| `0x45` | `01` `00 00 00 00` `86 4E 00 00` `05 00 00 00` | Position present: 0, 20102; orientation 5 |
| `0x52` | `00 00 00 00` | Empty waiting list |

**`state_hash(W1) = 121BBD7C8E51E03E`, `combat_hash(W1) = CDC183838522E1A7`.**

### 5.4 W2, W3 and W4

- **W2** is W0 with MVAR [1, -1] and two LVAR slices, supplied out of order:
  script `0x04000002` with [5], then script `0x01000007` with [0, 9]; its
  entities are also supplied as [`3:1`, `2:3`, `1:1`]. The encoder writes the
  entities as in W0 and the slices as `0x01000007` first. The MVAR part is
  `4D 56 41 52 01 02 00 00 00 01 00 00 00 FF FF FF FF`; the LVAR part is
  `4C 56 41 52 02 00 00 00`, then `07 00 00 01 02 00 00 00 00 00 00 00 09 00
  00 00`, then `02 00 00 04 01 00 00 00 05 00 00 00`. 332 bytes
  (`rev2/w2.stream.hex`); **`state_hash(W2) = 7AE39E9C0B29866D`**.
- **W3** is W0 after one draw from its generator: state 812987216
  (`0x30753350`), selector 0 (the output U was entry 0, which held 0), entry 0
  now 812987216; all other entries unchanged. 292 bytes
  (`rev2/w3.stream.hex`); **`state_hash(W3) = 04A0AD69522C3565`**.
- **W4** is W1 at round 1 with no current actor and a waiting list holding
  the stale handle `5:1`, which no entity has. The stale participant is the
  nine bytes `05 00 00 00 01 00 00 00 00`. 374 bytes (`rev2/w4.stream.hex`;
  combat stream `w4.combat.hex`, 91 bytes, unchanged); **`state_hash(W4) =
  02A2EFB95101DA7D`, `combat_hash(W4) = A8475FA8866A8966`**.

### 5.5 Single-Field Mutations

Each row changes one field of its base; the stream length does not change.
The same rows are in `rev2/hashes.tsv`. The `combat_hash` column is the same
as in revision 1.

| Vector | Base | Change | `state_hash` | `combat_hash` |
| --- | --- | --- | --- | --- |
| M-gvar | W0 | GVAR 0: 7 to 6 | `45A5547A2010CCB5` | `03A3252340D67E8D` |
| M-gen | W0 | Handle `2:3` to `2:2` | `5869818B802DD34D` | `03A3252340D67E8D` |
| M-hp | W0 | Entity `1:1` HP 30 to 31 | `E18ED4C360E852DB` | `03A3252340D67E8D` |
| M-hex | W0 | Entity `1:1` hex 20100 to 20101 | `39996D585A8710C9` | `03A3252340D67E8D` |
| M-inv | W0 | Entity `1:1` quantity 3 to 2 | `C954F4CCFFB267ED` | `03A3252340D67E8D` |
| M-rng | W0 | RNG entry 0: 0 to 1 | `5C0C6CC2E2B7A215` | `03A3252340D67E8D` |
| M-turn | W1 | Current turn 0 to 1 | `1FF653DEE3A223B3` | `C25648C507DC28BE` |
| M-order | W1 | Active list reversed to [`2:3`, `1:1`] | `08EBD88C824628F6` | `201A03C86DDF92BF` |

M-inv changes only the holder's inventory entry. The item entity `3:1` has no
quantity field, so its record does not change.

M-order shows that turn order is state (section 3.2), not a sorted set.

### 5.6 Trace Examples and Goldens

**VM trace.** `vm-trace-program.tsv` is the trace of SPEC-004 section 4.1's
282-byte program, 44 rows, stepped by hand from SPEC-004. Its first rows and
its last three:

```text
step	program	pc	word	imm	next_pc	vsp	rsp	fp	gp	top	status
0	program.int	00000000	8002	-	00000002	0	0	-1	-1	-	ok
1	program.int	00000002	C001	00000012	00000008	6	0	-1	-1	C001:00000012	ok
2	program.int	00000008	800D	-	0000000A	0	6	-1	-1	-	ok
3	program.int	0000000A	C001	00000084	00000010	6	6	-1	-1	C001:00000084	ok
4	program.int	00000010	8004	-	00000084	0	6	-1	-1	-	ok
5	program.int	00000084	802C	-	00000086	0	6	-1	0	-	ok
...
41	program.int	000000F8	8029	-	000000FA	6	6	-1	0	C001:00000063	ok
42	program.int	000000FA	801C	-	00000012	6	0	-1	0	C001:00000063	ok
43	program.int	00000012	8010	-	00000014	6	0	-1	0	C001:00000063	ended
```

The rows agree with SPEC-004 section 4.1's narrative: F becomes 6 at step 11
and again in the helper at step 19; the add at step 24 leaves `C001:0000000C`
(12); the branch at step 34 falls through; step 37 stores 99 in global slot
0; the program ends at step 43 with next address `0x14` (decimal 20), F -1, G
0, an empty return stack and a value stack holding only `00 00 00 63 C0 01`.

`vm-trace-program-branch.tsv` traces the branch mutation (the payload at
`0x00AE` set to 1): 42 rows. The helper returns 6, the comparison at step 33
leaves 0, the branch at step 34 goes to `0x00E8`, and the program ends at
step 41 with global slot 0 holding `FF FF FF FF C0 01`.

**Combat trace example.** `rev2/combat-trace-example.tsv` is a format
example built from four snapshots of the revision 2 worlds, not from a combat
run:

| Snapshot | Definition |
| --- | --- |
| S0 | W1 with no current turn |
| S1 | W1 with entity `1:1` at hex 20101, orientation 1, AP 0 |
| S2 | S1 with current turn 1, entity `1:1` HP 26, entity `2:3` AP 0, and W3's RNG record |
| S3 | S2 with no combat and entity `1:1` AP 8 |

```text
seq	event	round	turn	actor	combat_hash	state_hash
0	start	0	-	-	A27F1ECCE54073B8	8FFF0E4AD482A9C1
1	turn	0	0	1:1	8CE41A4BD1A0B4BD	72093C17B0E94812
2	turn	0	1	2:3	98A9D307A01B8F76	04C6CFD75DBFB7C6
3	end	-	-	-	03A3252340D67E8D	866BE03FF440F079
```

Only the `state_hash` column differs from revision 1's
`combat-trace-example.tsv`, because every snapshot now holds the item entity.

**Seeded combat golden.** `combat-trace-synthetic-seed1.tsv` is recorded by
the implementer (section 3.8, test T11). Revision 2 requires it to be
re-recorded with the AI fixture of section 5.7 installed, and revision 3 adds
the art fixture of section 5.8 (section 3.9).

### 5.7 The Combat AI Fixture (Revision 2)

`rev2/ai.txt` is a complete SPEC-009 `ai.txt` with one packet. It is ASCII
with LF line ends (SPEC-009 section 2.2 trims the CR of a CRLF file the same
way), and no line is longer than 255 bytes. The full file:

```text
; SPEC-021 revision 2: synthetic combat AI packet for the seeded combat golden.
; Written for Vault Engine by Team A. Not game data. See SPEC-021 section 5.7.
[Synthetic Brawler]
packet_num=0
max_dist=500
min_to_hit=-1000
min_hp=0
aggression=100
hurt_too_much=
secondary_freq=1000
called_freq=1000
font=101
color=0
outline_color=-1
chance=0
run_start=13
move_start=12
attack_start=11
miss_start=10
hit_head_start=9
hit_left_arm_start=8
hit_right_arm_start=7
hit_torso_start=6
hit_right_leg_start=5
hit_left_leg_start=4
hit_eyes_start=3
hit_groin_start=2
last_msg=1
```

It parses to one packet, `Synthetic Brawler`, holding all 25 keys of
SPEC-009 section 2.3, so initialization succeeds (section 3.1). The two
comment lines start with `;` and are empty after truncation (section 2.2).
`hurt_too_much=` has an empty value, which yields mask 0 with no diagnostic
(section 2.4).

`rev2/combatai.msg` is the one-record SPEC-002 catalog `{1}{}{Synthetic
taunt.}` followed by LF (bytes `7B 31 7D 7B 7D 7B 53 79 6E 74 68 65 74 69 63
20 74 61 75 6E 74 2E 7D 0A`). It exists only because AI initialization loads
`combatai.msg` first and fails without it (SPEC-009 section 3.1). This packet
never looks a line up in it (below).

**Why the NPC fights.** The NPC defender's turn follows SPEC-009 section 3.6:

| Value | Rule it controls | Effect in the trace |
| --- | --- | --- |
| One packet only | Packet resolution, section 3.1 | Every critter resolves to this packet: packet number 0 matches `packet_num=0`, and any other number falls back to table entry 0, which is this packet, with a diagnostic. No critter field needs changing |
| `min_hp=0`, `hurt_too_much=` (mask 0) | Forced flee, section 3.6 step 1 | Never taken. A living critter has HP of at least 1, so HP below 0 never holds, and flags AND 0 is 0. Combat start keeps only alert bit `0x01` (SPEC-007 section 3.3), so the fleeing bit is clear |
| (no packet value) | Section 3.6 step 2 and danger source, section 3.5 | The trace's request comes from the driver, not a script, and its first turn is the player's (SPEC-007 section 3.3: attacker first). So the NPC has no requested target. It heals only if it carries a stimpak, which uses no draw. Combat start made the player and the NPC each other's last attacker, through the setter (SPEC-009 section 3.4, first row). The player is alive, so `danger(self)` returns the player at once, with no perception test. The NPC then runs the attack loop against the player |
| `min_to_hit=-1000` | Weapon usability, section 3.9; the attack loop, section 3.12 | The hit chance has no lower cap (SPEC-007 section 3.5, step 10), so a threshold of 0 could still make a weak NPC flee or refuse to attack. The loop compares only the range-aware chance of an attack already in range, and the no-range chance, which has no distance term. For stats in their legal ranges, neither comes near -1000. So every comparison with `min_to_hit` passes. In range (preflight 0) the NPC **attacks**; out of range (preflight 2) the no-range test cannot send it fleeing, so it **approaches**, after a weapon switch when it holds no weapon. The skill test of weapon usability always passes |
| `max_dist=500` | Max-distance stop, section 3.6 step 4; flee, section 3.8 step 1 | Never reached: 500 is more than any hex distance on SPEC-003's 200 by 200 grid, so the NPC never sets the stop bit for distance |
| `chance=0` | Taunts, section 3.17 step 4 | The 1-100 chance draw is still made at each request site (section 3.19), but it always exceeds 0, so no taunt is shown and `combatai.msg` is never read for a line |
| Range keys from 13 down to 1 | Taunt ranges, section 3.17 steps 5 and 6 | Every range ends one below its start, so it is empty even if `chance` is edited. No line draw is ever made |
| `secondary_freq=1000`, `called_freq=1000` | Sections 3.10 and 3.11 | Secondary modes and called shots are possible but rare: each gate makes its one draw and passes only on a 1. The draws are part of the golden |
| `aggression=100`, `font=101`, `color=0`, `outline_color=-1` | Required keys only | `aggression` is never consulted, and the font and colors are used only for a taunt, which never shows |
| (no packet value; revision 3) | Unarmed upgrade, SPEC-009 section 3.12 step 3; the attack step, section 3.12 "Attack" sub-step 4 | The NPC holds no weapon, so it punches. The punch is an attack only if the SPEC-007 attack request succeeds, and the request succeeds only if the attack's animation sequence registers (SPEC-007 section 3.10.1). That needs the punch art and the art of every reaction the sequence holds. The art fixture of section 5.8 supplies them. With the punch art present, step 3's art test passes, so no weapon switch runs on that ground |

Revision 2 checked every packet gate but not the art the attack sequence
loads; the Wave 4 run stopped there (section 5.8). With both fixtures
installed, the NPC's first turn starts at distance 1 from the player, with
preflight 0 and a hit chance that passes `min_to_hit`. Each punch whose
sequence registers debits 3 AP (SPEC-007 section 3.4). Team A expects the NPC
to punch while its AP covers the cost: AP 7 before the first punch, 4 after
it, and 1 after the second, when nothing interrupts. But the golden records
what happens; it is not predicted here.

Once the NPC has spent AP, it no longer wants to stop (SPEC-009 section 3.14,
step 3). Combat ends only by SPEC-007 section 3.3's automatic end or a death.
Whether a punch lands, and for how much, depends on SPEC-007's rolls, so Team
A cannot predict the trace rows. That is why the golden is recorded (D-24). T11
checks only that the NPC acts (section 4).

### 5.8 The Critter Art Fixture (Revision 3)

**The Wave 4 finding, recorded as measured fact.** The implementer reported
this run, relayed by the coordinator. Team A did not re-run it.

- With `rev2/ai.txt` and `rev2/combatai.msg` installed, the seeded combat
  trace on `maps/synthetic.map` with seed 1 still had the defender `3:1`
  spend no AP. T11's "NPC acts" assertion failed, as section 3.9 step 5's
  stop rule anticipated.
- Everything before the attack worked. The packet loaded. Section 3.6 step 1
  of SPEC-009 forced no flee, and `danger()` returned the player. The
  preflight was 0, and the hit chance of 58 passed `min_to_hit=-1000`. The
  defender had AP 7 of 7, at distance 1 from the player.
- SPEC-009 section 3.12 step 3 ran weapon switching, because the punch art
  was missing. The NPC owns no weapon, so the punch mode stayed.
- The blocking step was SPEC-009 section 3.12, "Attack" sub-step 4. The
  SPEC-007 attack request was refused because the attack's animation sequence
  could not register (SPEC-007 section 3.10.1). The art identifier
  `0x01100000` was missing: category 1 (critter), animation 16 (punch),
  weapon code 0, base index 0, file selector 0.
- The refusal comes before the AP debit, so AP stayed at its maximum.
- For base `sample` the corpus has only `art/critters/sampleaa.frm` (stand),
  `sampleak.frm` and `sampleal.frm` (animations 10 and 11), and no punch.

Team A's own listing of the corpus agrees: `art/critters/critters.lst` is
the single row `sample,0`, so base index 0 is `sample` and no other base
exists. The only critter prototype stores art identifier `0x01000000`. So
the player's art also names `sample`, the base this fixture supplies. A
critter whose base index is past the end of the list names no file at all
(SPEC-007 section 2.1).

**Why one punch file is not enough.** A punch registers one sequence
(SPEC-007 section 3.10.3). On a hit, or when the critical bit is set, it
holds the attack animation and then the reactions of section 3.10.6. Each
reaction is an animate entry whose art must load, or the whole sequence is
discarded (section 3.10.1, "Failure"). With a hit chance of 58, most punches
hit. So the punch file alone would only move the failure from the attack
animation to the defender's reaction. The table lists every reaction row
that a bare punch between two `sample` critters can reach. A punch does
normal damage, so no fire row applies. `[Must-match]`

| SPEC-007 section 3.10.6 row | Animations it registers | Covered by |
| --- | --- | --- |
| R prone when the sequence is built | Blood only, and only when its art exists; none does | Nothing needed |
| Dead, fire dance 33 | Not reachable: 33 needs fire damage (section 3.8) | Nothing needed |
| Dead, knockback, death animation 20 or 21 | 20 or 21 in the knockback executor; blood only when its art exists | `sampleba.frm`: fall-front without art falls back to fall-back (section 3.8) |
| Dead, otherwise | The death animation of section 3.8, then the fall choice | `sampleba.frm`: a severe choice without art falls back to 20 on a front hit, and to 21 on a rear hit (the reference's inverted fallback). The reaction's fall choice then replaces the missing fall-front 21 with 20 |
| Knocked out or knocked down | 20 or 21 after the fall choice; the critter stands up on its own turn | `sampleba.frm`, then `samplecj.frm` for the stand-up (37 after 20; SPEC-007 section 3.3) |
| On fire, with art for 33 | Not reachable: no art for 33, and a punch sets no fire | Nothing needed |
| Knockback (k not 0) | 20 or 21 and the knockback executor, then 37 after 20 | `sampleba.frm`, `samplecj.frm` |
| Otherwise (hit flinch) | 14, or 15 when the hit is not from the front and art for 15 exists | `sampleao.frm`; there is no art for 15, so it is always 14 |

The other entries need nothing new:

- **Critical failures.** An unarmed attacker uses critical-failure table 0
  (SPEC-007 section 2.4). Its masks are 0, `8000` (lose turn), `80002`
  (self injury, knocked down) and `200000` (random cripple). The attacker's
  own reaction is then the hit flinch or the knockdown row: `sampleao.frm` or
  `sampleba.frm`. No mask of table 0 explodes a weapon, so no set-art entry
  is registered.
- **Misses.** `sample` has no dodge art (animation 13, `an`). So a miss
  registers no attack animation in the reference, or the punch under its
  `[May-improve]` (section 3.10.3). Either way the sequence registers.
- **Turn and standing reset.** The turn entry and the standing reset at the
  end of a sequence use the standing art `sampleaa.frm`, which the corpus
  has.

**File names.** Each name follows from SPEC-007 sections 2.1 to 2.3, with base
index 0 (`sample`) and file selector 0. The suffixes of animations 14, 16, 20
and 37 do not depend on the weapon code (section 2.2), so the same file serves
a critter with any weapon code. None of these codes is aliased (aliases apply
only to 27, 29, 30, 33, 55, 57, 58 and 64). `[Must-match]`

| Animation | Use | Art identifier (weapon code 0) | Suffix | Installed at |
| ---: | --- | --- | --- | --- |
| 16 punch | The NPC's attack animation (section 3.10.2) | `0x01100000` | `aq` | `art/critters/sampleaq.frm` |
| 14 hit from front | Hit flinch | `0x010E0000` | `ao` | `art/critters/sampleao.frm` |
| 20 fall back | Knockdown, knockout, knockback and death falls | `0x01140000` | `ba` | `art/critters/sampleba.frm` |
| 37 back to standing | Stand-up after a fall back | `0x01250000` | `cj` | `art/critters/samplecj.frm` |

Animations 14, 16 and 37 encode selector 0 at once (section 2.3 rule 1).
Animation 20 first probes the directional files (rules 2 and 3). The probes
find no `sampleba.fr0` to `sampleba.fr5`, so it encodes selector 0 and names
`sampleba.frm`.

**The bytes.** The four files are byte-identical, 75 bytes each, built by
SPEC-001 sections 2.2 and 2.3. Every multi-byte field is big-endian.

```text
0000: 00 00 00 04 00 0A 00 00 00 01 00 00 00 00 00 00
0010: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
0030: 00 00 00 00 00 00 00 00 00 00 00 00 00 0D 00 01
0040: 00 01 00 00 00 01 00 00 00 00 10
```

| Offset | Bytes | Meaning |
| --- | --- | --- |
| `0x00` | `00 00 00 04` | Version word 4 |
| `0x04` | `00 0A` | 10 frames per second: a frame step every 100 ms (SPEC-001 section 3.4) |
| `0x06` | `00 00` | Action frame 0 |
| `0x08` | `00 01` | One frame in each direction sequence |
| `0x0A` | 12 x `00` | Six X shifts, all 0 |
| `0x16` | 12 x `00` | Six Y shifts, all 0 |
| `0x22` | 24 x `00` | Six direction offsets, all 0: all six directions share one sequence (SPEC-001 section 3.3) |
| `0x3A` | `00 00 00 0D` | Frame area 13 bytes: one 12-byte record header and one pixel |
| `0x3E` | `00 01` `00 01` | Frame 0: width 1, height 1 |
| `0x42` | `00 00 00 01` | Pixel-data length 1 (width times height) |
| `0x46` | `00 00` `00 00` | Frame displacement (0, 0) |
| `0x4A` | `10` | The pixel: palette index `0x10`, the index the corpus's own critter art uses; not the transparent index 0 (SPEC-001 section 3.1) |

The file passes SPEC-001 section 3.3's `[May-improve]` checks: direction 0's
offset is 0, every later offset equals the one before it, the frame count is
positive, and the payload equals width times height and ends at the end of
the file.

What follows from the bytes (SPEC-007 section 3.10):

- An animate entry with this art takes two frame steps: one shows frame 0,
  the next finishes the entry.
- The punch's action frame A is 0. So its contact sound and the reactions
  start in the same examination as the attack animation.
- The pixels and the timing change no covered value (section 3.1). They do
  change when the trace's turns end, and the golden records that.

**Deliberately absent.** Each file below is left out on purpose. `[Must-match]`

- **`sampleap.frm` (15, hit from back).** Without it, every flinch is 14.
- **`samplebb.frm` (21, fall front).** Without it, every fall resolves to
  20, so every stand-up is 37. That is also why `samplech.frm` (36) is not
  needed.
- **`samplean.frm` (13, dodge).** Without it, a miss takes the "no dodge
  art" branch.
- **`samplear.frm` (17, kick).** The punch-to-kick draw is still made, one
  draw per punch request (SPEC-007 section 3.6). Its probe for the kick art
  finds nothing, so the hit mode stays punch.
- **`sampleab.frm` (1, walk).** The defender starts adjacent to the player,
  so its first turn attacks without moving. A walk needs frames with
  displacements for SPEC-007 section 3.11.1's hex-crossing rule, so a
  one-frame walk would not be a meaningful fixture. A later approach is needed
  only after a knockback, which takes at least 10 total punch damage
  (section 3.8). That approach then fails as SPEC-009 section 3.12 says, the
  same way on every run, and the golden records it.

### 5.9 Checkpoint Vectors (Revision 4)

`rev3/checkpoints-example.tsv` is a format example built from the revision 2
worlds, not from a run. Each hash is in `rev2/hashes.tsv`.

```text
vault-checkpoints	1	VSH1	VCH1
tick	input_index	state_hash	combat_hash
0	0	0258CFED4EA8A13E	-
1	2	0258CFED4EA8A13E	-
2	3	04A0AD69522C3565	-
3	3	121BBD7C8E51E03E	CDC183838522E1A7
4	4	1FF653DEE3A223B3	C25648C507DC28BE
5	4	02A2EFB95101DA7D	A8475FA8866A8966
6	6	04A0AD69522C3565	-
```

| Row | State | Reading |
| ---: | --- | --- |
| 0 | W0 | The first row: no record applied, no combat |
| 1 | W0 | Two records applied, and nothing covered changed |
| 2 | W3 | One more record, and one random draw |
| 3 | W1 | Combat is active, so `combat_hash` is written. No new record |
| 4 | M-turn | The turn moved: both hashes change |
| 5 | W4 | Round 1, with a stale waiter |
| 6 | W3 | Combat is over, so `combat_hash` is `-` again |

Row 6 shows the rule of section 3.10: the constant `03A3252340D67E8D` of section
2.7 is never written, although it is W3's `combat_hash`.

`rev3/compare-cases.tsv` holds 15 comparisons against that file as the expected
stream. A row at `tick` t is on line t + 3 of the file.

| Case | Change to the actual copy | Verdict |
| --- | --- | --- |
| `same` | None | Match, 7 rows |
| `state` | Tick 2's `state_hash` is M-gvar's | Mismatch at tick 2 |
| `combat-absent` | Tick 3's `combat_hash` is `-` | Mismatch at tick 3 |
| `input` | Tick 1's `input_index` is 3 | Mismatch at tick 1 |
| `first-of-two` | Ticks 2 and 4 differ | Mismatch at tick 2 only |
| `first-column` | Two columns of tick 3 differ | Mismatch names `state_hash` |
| `short` | Ends after tick 4 | Ended early |
| `long` | A row for tick 7 is added | Too long |
| `version`, `magic`, `header` | Line 1's version, line 1's `VSH2`, or a renamed column | Format mismatch; no row is read |
| `lowercase`, `tick-gap`, `input-back`, `columns` | A lowercase hash, a `tick` of 9, a falling `input_index`, or a missing column | Malformed |

### 5.10 The Scenario Logs (Revision 4)

Both logs use only the records `surface`, `tick`, `pointer_moved`,
`pointer_button`, `key` and `quit`, on a 640-by-480 surface with a 16 ms frame.
Each starts with `surface` and ends with `quit`. A pointer record is followed by
a `tick`, and so is each press and each release. These phases are what the logs
do, as measured by Team A on the binary of 2026-10-30, by the final frame of a
replay cut off after each phase. The goldens and the assertions of section 3.11
are what bind. `[Must-match]` for the log bytes.

`rev3/scenario-explore-talk.jsonl`, 403 records and 387 steps:

| Steps | Input | What happens |
| ---: | --- | --- |
| 1-10 | None | The map settles |
| 11-133 | Left click at (288, 205), in movement mode | The player walks, and the view follows it by about (+16, -8) |
| 134-138 | Key `KeyM` | The cursor mode becomes the arrow |
| 139-261 | Left click at (352, 161), on the Guide | The Guide's conversation opens |
| 262-324 | Left click at (130, 340), on the option | The first option is chosen, and the conversation stays open |
| 325-387 | Left click at (130, 340), on the option | The conversation ends, and the map returns |

`rev3/scenario-world-travel.jsonl`, 622 records and 615 steps:

| Steps | Input | What happens |
| ---: | --- | --- |
| 1-5 | None | The map settles |
| 6-12 | Key `KeyW` | The world map opens, with the party at (825, 75) |
| 13-615 | Left click at (407, 96) in the world view, 160 pixels east of the party | The party travels, and arrives with no encounter. The final frame is the same at step 615 and 300 steps later |

The world view is at screen (22, 21) (SPEC-008 section 3.4), with the scroll
origin at X 600, so world X 825 is screen X 247.

**Why (c) has no log.** Team A tried one. `KeyA` starts combat, but with no
attack a first Space ends it, and the frame does not change with the seed. After
`KeyA`, `KeyM` twice reaches the crosshair, but a left click on the Guide then
spends no AP, and the digest does not change over the next 230 steps (measured
by Team A). T11's driver already makes the NPC punch on each of its turns, as
the committed golden shows, so scenario (c) uses it (D-48).

## 6. Decisions

Every open point of the brief is settled here.

- **D-1 (owner instruction).** Covered items: entity handles (index and
  generation), hex coordinates, orientations, HP/AP, inventory counts, the
  RNG state, the combat turn queue, and GVAR, MVAR and LVAR. The method is
  `WorldState::state_hash(&self) -> u64`.
- **D-2 (owner instruction).** A fully specified algorithm over a canonical
  byte stream, with stable collection order. The hash is public and fully
  specified; std's `Hash` and `DefaultHasher` are not used.
- **D-3 (Team A default).** The hash function is FNV-1a 64, the owner's
  example. Its single-byte property (section 2.1) makes the single-bit test
  certain rather than probabilistic, and it needs no dependency.
- **D-4 (owner instruction).** A separate combat sub-hash, by the same rules.
- **D-5 (owner instruction).** Two TSV traces and an `asset-audit` command to
  dump them; goldens in `tests/fixtures/spec021/`, not in `oracle/`.
- **D-6 (owner instruction).** The tests of section 4 include the
  identical-seed and single-bit tests (T7, T4, T8), and the vectors are
  synthetic (section 5).
- **D-7 (Team A default).** Little-endian fixed-width integers, no
  platform-width values and no floats. Little-endian matches Rust's
  `to_le_bytes` on every supported host; the choice is arbitrary but fixed.
- **D-8 (Team A default).** Four-byte ASCII tags start the stream and each
  part, and the magic carries a revision digit. The tags make a stream dump
  readable and separate the parts, so that bytes cannot move from one part to
  the next without changing the stream.
- **D-9 (Team A default).** An absent value is the byte `00`; a present one
  is `01` and the value. No sentinel value (such as -1) stands for absence.
- **D-10 (Team A default).** Entities are sorted by slot index, then
  generation. Allocation order is therefore part of the state (section 3.2).
- **D-11 (Team A default).** Sequences whose order has gameplay meaning (the
  turn queue, the waiting list, variable arrays, the RNG entries) are written
  in that order; keyed collections are sorted.
- **D-12 (Team A default).** The entity record carries the PID, beyond the
  brief's list. Without it the critter rule and the inventory entries would
  not be self-describing, and an entity whose prototype changed would hash
  the same.
- **D-13 (Team A default).** HP and AP are present exactly for PID type 1
  (critters), and absent for every other entity.
- **D-14 (Team A default).** "Inventory counts" means each direct inventory
  entry's item PID and quantity. Nested container contents are covered only
  through entity records; if the engine does not store such containers as
  entities, their contents are a known gap of revision 1.
- **D-15 (Team A default).** A position is elevation plus SPEC-003's linear
  hex number, and is absent for an entity not on the map.
- **D-16 (Team A default).** The RNG record is SPEC-006's recurrence state,
  raw selector and 32 shuffle entries. Draw counters and the host entropy
  source are not covered.
- **D-17 (Team A default).** The combat record holds a round counter, the
  current turn position, and the active and waiting lists, each participant
  with its HP, AP, position and orientation. A combat state that does not yet
  count rounds gains a counter for this purpose. Free movement, last
  opponents, accumulated damage, maneuver bits and the pending attack are not
  covered by revision 1.
- **D-18 (Team A default).** LVAR slices are keyed by script identifier, only
  non-empty slices are written, and duplicate identifiers are ordered by
  their contents.
- **D-19 (Team A default).** Everything outside the brief's list (section
  3.1) is uncovered in revision 1, to keep the hash's meaning exactly the
  owner's. A later revision may extend coverage under a new magic.
- **D-20 (Team A default).** The VM trace has the columns of section 3.5,
  including the frame and global bases F and G, which SPEC-004 defines and
  which the section 4.1 narrative checks. Hexadecimal fields are uppercase
  and fixed-width; heights are byte heights, as SPEC-004 measures them.
- **D-21 (Team A default).** The VM trace interleaves every program's
  instructions in execution order and names each row's program; the program
  name is the lowercase file name without `.hex`.
- **D-22 (Team A default).** The combat trace writes a row at combat start,
  after every turn and after cleanup. The player always ends its turn at
  once, and the defender is the lowest-handle eligible critter. These choices
  make the driver deterministic and need no input script.
- **D-23 (Team A default).** The `asset-audit` flags are `--dump-vm-trace`,
  `--dump-combat-trace`, `--max-steps`, `--max-rounds` and `--trace-out`,
  with the existing `--corpus-dir` and the same `--seed` meaning as
  `vault-platform`. Dump modes are exclusive with the validation and report
  flags. Revision 1 traces run under the Fallout 1 profile only.
- **D-24 (Team A default).** Team A cannot compute a combat run by hand, so
  the seeded combat golden is recorded once by the implementer and then
  frozen, under SPEC-020 section 3.4's re-recording rule. Every other golden
  in this spec was computed from the spec alone.
- **D-25 (Team A default).** The combat sub-hash is computed by `WorldState`,
  because its participants read entity data. The world stream embeds the
  combat stream's bytes after its magic, so the two hashes cannot disagree
  about what the combat state is.
- **D-26 (Team A default).** The interface names of section 3.3
  (`combat_hash`, `state_snapshot`, `StateSnapshot`, `canonical_bytes`,
  `canonical_combat_bytes`, `fnv1a64`) are Team A's. `state_hash` is the
  owner's.
- **D-27 (Team A default).** Hash values in traces and tables are 16
  uppercase hexadecimal digits, without a prefix.

Revision 2 adds these decisions. The coordinator relayed the two open items
and the implementer's findings; the choices below are Team A's.

- **D-28 (Team A default).** Open item 4 is resolved by adding the held item
  to W0 as entity `3:1`, with no position, rather than by dropping W0's
  inventory entry. The stream format already covered held items; only the
  vector was unrepresentable. The format and the magic therefore stay `VSH1`
  (section 2.3 requires a new magic only for a change of coverage or
  encoding). The handle `3:1` is the one the implementer's allocator gives
  the item, so T5 can check W0 directly.
- **D-29 (Team A default).** W1 to W4, the mutation rows and the combat trace
  example are rebased on the new W0, so that no revision 2 vector holds an
  item without an entity. The mutation rows keep their revision 1 changes, so
  the two sets can be compared row by row.
- **D-30 (Team A default).** Revision 2 files go to the new folder
  `tests/fixtures/spec021/rev2/`. The merged revision 1 tests read the top
  folder, and Team A does not change code, so editing the files in place
  would break the build until the follow-up lands. The implementer's
  follow-up switches the tests and deletes the superseded files (section 3.9).
- **D-31 (Team A default).** Entity records with equal handles are ordered by
  their remaining bytes. Revision 2's W0 is the first vector where T4's flips
  can make two handles equal, and without a rule the stream would depend on
  supply order, which section 3.2 forbids.
- **D-32 (Team A default).** Open item 5 is resolved with a single-packet
  `ai.txt` and a one-line `combatai.msg`, installed by the combat trace's
  test setup rather than by `cargo xtask fixtures`, so the shared synthetic
  corpus and every other test that reads it stay unchanged. Packet values are
  chosen so that every SPEC-009 gate that could make the NPC flee, stop or
  stay idle is closed (section 5.7). Taunts are turned off with `chance=0`
  and empty ranges, so the trace does not depend on the catalog's text.
- **D-33 (Team A default).** The combat trace dump refuses to run without a
  loaded packet table, and T11 asserts that the NPC spends AP. Together they
  stop a golden from being recorded with an idle NPC again.

Revision 3 adds these decisions. The coordinator relayed the implementer's
Wave 4 finding and asked for the smallest fix inside SPEC-021's own fixtures;
the choices below are Team A's.

- **D-34 (Team A default).** The Wave 4 finding (section 5.8) is recorded as
  measured fact, from the implementer's report. Team A did not re-run the
  trace. It only listed the corpus's critter art files, its critter list and
  its prototype's art identifier, to confirm that the base is `sample` for
  both combatants.
- **D-35 (Team A default, on the coordinator's preferred option).** The
  blocker is fixed with synthetic critter art that T11's setup installs, like
  the AI fixture (D-32). `cargo xtask fixtures` and the shared synthetic
  corpus do not change, so no other test or audit count moves. The dump
  command still reads the corpus as it is. It does not check for the art,
  because T11's assertion and stop rule already catch a missing install.
- **D-36 (Team A default).** Four files, not one: the punch (16), the hit
  flinch (14), the fall back (20) and the stand-up (37). The punch alone
  would only move the registration failure to the defender's reaction,
  because every reaction animation's art must load too (SPEC-007 section
  3.10.1). These four cover every reaction row that a bare punch between two
  `sample` critters can reach, including critical failures from table 0, as
  section 5.8 shows. The stand-up file covers the turn after a knockdown.
- **D-37 (Team A default).** Art that would widen the scenario is left out on
  purpose: fall front, hit from back, dodge, kick and walk (section 5.8,
  "Deliberately absent"). Without fall front, every fall resolves to 20 and
  every stand-up to 37. Without kick, the hit mode stays punch. The walk is
  not needed for the first NPC turn, and a one-frame walk could not show the
  hex-crossing rule.
- **D-38 (Team A default).** The four files are byte-identical, 75 bytes:
  one 1-by-1 frame of palette index `0x10`, all six directions sharing it,
  10 fps and action frame 0. They are stored as raw `.frm` files in
  `tests/fixtures/spec021/rev2/`, beside the AI fixture, because T11 copies
  them byte for byte. Section 5.8 dumps every byte. No revision 2 vector
  changes: art and animation are not covered by the hash (section 3.1), so
  every stream and hash of section 5 stands. Only `rev2/README.md` gains
  their description.
- **D-39 (Team A default).** The seeded golden is recorded only with both
  fixtures installed (section 3.9). T11's stop rule now also asks the report
  to name the SPEC-007 rule and the missing resource, and it covers a
  defender whose art names a base other than `sample`.

Revision 4 adds these decisions.

- **D-40 (Team A default; the folder name is an owner instruction).** The
  owner's brief of 2026-10-30 names this work "Revision 3". Revision 3 was
  approved on 2026-10-27 and has been implemented, so this is revision 4, which
  keeps the history true. The brief's folder `tests/fixtures/spec021/rev3/` is
  kept as written: it was free, because revision 3 kept its files in `rev2/`
  (D-38). The folder's README says so.
- **D-41 (owner instruction).** A checkpoint is recorded in
  `vault_platform::session`'s headless step (SPEC-020 section 2.3), before
  rendering (SPEC-022 section 3.13). Placing it after the simulation of its own
  step is a Team A default: the row then describes the state the frame draws,
  and a change appears in the row of the step that made it.
- **D-42 (Team A default).** Row 0 is written before the first step, so that a
  divergence in session setup (map load, seed, script start) is seen before any
  input. N steps give N + 1 rows.
- **D-43 (owner instruction for the columns; Team A default for the rest).**
  Columns `tick`, input-record index, `state_hash`, and `combat_hash` or `-`.
  Line 1 carries a format version and both stream magics, so that a change of
  coverage cannot be compared by mistake. `-` replaces the no-combat constant
  because it says what the row means, and an active combat can never be mistaken
  for none.
- **D-44 (owner instruction for failing at the first differing row; Team A
  default for the report text and the check order).** The exact report texts are
  fixed so that a test can compare them. A row after a divergence is expected to
  differ, so counting differences would say nothing. `--checkpoints-expect`
  compares while the replay runs, so it stops at the step that diverged.
- **D-45 (Team A default).** The flags are `--checkpoints-out` and
  `--checkpoints-expect`, on `vault-platform` for replays and on `asset-audit`
  for the driver. Status 2 is the status `asset-audit` already gives a failed
  trace (section 3.7).
- **D-46 (owner instruction).** This stream is the lockstep synchronization unit
  for future multiplayer (SPEC-025). The two limits named in section 3.10 are
  Team A's, so that SPEC-025 does not assume more of this hash than section 2.1
  and section 3.1 promise.
- **D-47 (Team A default).** Revision 4 keeps `VSH1` and section 3.1's coverage.
  Covering world-map travel would need a new magic and would re-record every
  golden that holds a state hash, which the brief does not ask for. Scenario (b)
  covers that gap with its own final row instead, and through the encounter
  draws.
- **D-48 (Team A default).** Scenarios (a) and (b) are input logs, the replay
  path the brief names. Scenario (c) uses the section 3.7 driver, because a
  log-only attack does not start a fight on this corpus (section 5.10), and the
  driver's combat is the one T11's golden already fixes. A log-driven fight
  needs a spec revision and a reason why the click spends no AP. `input_index`
  counts the driver's commands, so that the format stays the same.
- **D-49 (Team A default).** The three goldens are recorded by the implementer
  and then frozen, as D-24 did for the seeded combat golden, because Team A cannot
  compute them by hand. Team A's black-box runs only showed that each scenario
  can run. No output of those runs is committed as a golden. Each assertion of
  section 3.11 is a stated expectation, and the stop rule applies if one fails.

## 7. Clean-Room Verification Checklist

- [x] Zero C/C++ code snippets or struct definitions included.
- [x] No implementation-source file path (the `CLAUDE.md` ingestion rule),
  and no 40-character hexadecimal string. Engine types are named only as `WorldState`, the combat state and
  the RNG.
- [x] Every multi-byte integer's width and endianness stated (section 2.2).
- [x] Every rule tagged `[Must-match]`, `[May-improve]` or `[Safe rule]`;
  tables are tagged once and their rows inherit the tag.
- [x] Synthetic test vectors only; the RNG values follow from SPEC-006's own
  seed vector, and the VM traces from SPEC-004's synthetic program.
- [x] Team A computed every vector in section 5 from this spec with a
  throwaway script that is not committed, and checked the RNG values against
  SPEC-006 section 4.4 and the VM traces against SPEC-004 section 4.1.
- [x] Revision 2: the throwaway encoder first reproduced every revision 1
  file byte for byte, then wrote the `rev2/` vectors. A second, separate
  script then read only the written `rev2/` files. It checked the hex dump
  offsets, recomputed every FNV-1a 64 value from those bytes, and checked
  each revision 2 stream against its revision 1 stream with the count raised
  and the 23-byte item record inserted. It also checked the mutation rows and
  the trace snapshots by patching single fields in the written bytes. It
  re-ran all 1800 and 1864 T4 flips (distinct with and without the D-31
  rule, and from both supply orders), and parsed `rev2/ai.txt` by SPEC-009
  section 2.2's grammar (one packet, all 25 keys).
- [x] Revision 2 keeps the ingestion rule: no implementation-source path, no
  40-character hexadecimal string, and no code listing. No engine Rust code
  was read; a search of file names only confirmed that no build script
  parses this spec.
- [x] Decisions separate owner instructions from Team A defaults.
- [x] Revision 3: the four art files were written by a throwaway script from
  SPEC-001 section 2.2's header layout and section 2.3's frame record. A
  second, separate script read only the written files. It decoded each by
  SPEC-001 (version 4, 10 fps, action frame 0, one frame, zero shifts, six
  zero offsets, area 13, a 1-by-1 frame with payload 1 and pixel `0x10`). It
  checked that each file is 75 bytes, that the file ends where the frame
  ends, and that the four files are identical. It also decoded the art
  identifiers `0x01100000`, `0x010E0000`, `0x01140000` and `0x01250000`, and
  the corpus's `0x01000000`, `0x010A0000` and `0x010B0000`, to file names by
  SPEC-007 sections 2.1 to 2.3. That reproduced `sampleaq.frm`,
  `sampleao.frm`, `sampleba.frm`, `samplecj.frm` and the corpus's own
  `sampleaa.frm`, `sampleak.frm` and `sampleal.frm`.
- [x] Revision 3 keeps the ingestion rule: no implementation-source path, no
  40-character hexadecimal string, and no code listing. No engine Rust code
  and no fixture-builder code was read.
- [x] Revision 4: a throwaway script wrote `rev3/checkpoints-example.tsv` from
  `rev2/hashes.tsv`, and `rev3/compare-cases.tsv` and the two logs. A second,
  separate script read only the written files. It checked the example's bytes
  and traced every hash back to `rev2/hashes.tsv`. It applied each of the 15
  changes to a copy and ran its own comparison, which reproduced every verdict
  and report (the first run of this check found an off-by-one in the cases'
  line numbers, which was fixed in the generator). It also checked each log's
  record count, its consecutive command identifiers, its clock and its record
  kinds. Replaying the committed log bytes gave the same digests as the runs
  that measured each phase.
- [x] Revision 4 keeps the ingestion rule: no implementation-source path, no
  40-character hexadecimal string, and no code listing. No engine Rust code
  and no fixture-builder code was read. The black-box runs are disclosed in the
  provenance, and their findings are labelled "measured by Team A".
