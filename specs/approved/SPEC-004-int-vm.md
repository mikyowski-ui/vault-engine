# SPEC-004: Compiled Script Files and Core Virtual Machine

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, or internal variable names from `fallout1-re`.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes `.INT` compiled SSL scripts, their bootstrap and metadata tables, tagged values, the two-stack execution model, call frames, core instructions, and the six requested public lifecycle entry names. Evidence is static inspection of the interpreter, symbol registry, extension registration, and lifecycle dispatch in `fallout1-re` reference revision R1, targeting 32-bit Windows/x86. Binary layout observations were also checked against 952 scripts in the adjacent local corpus. Synthetic fixtures are independent of shipped script content.

Two scope corrections are necessary for an exact specification:

- **The core instruction range is `0x8000` through `0x804B`, not just through `0x802F`.** In particular, `0x8030` is a loop branch, `0x8031` and `0x8032` access local variables, and `0x8033` through `0x8046` include comparisons and arithmetic. General interpreter-library extensions begin at `0x804C`; game-specific extensions begin at `0x80A1`. Their full catalogs are deferred.
- **There is no defined or interpreted pure-procedure flag in this reference.** The verified procedure bits end at `0x10`. No bit number or purity guarantee is invented here.

The public lifecycle names are identifier-table strings used by the host to select procedures, not instruction words. Their host event numbers are also not bytecode opcodes. This document includes no implementation identifiers or implementation-source paths. Results labeled as recovered quirks are source-derived; original-executable behavior has not been independently measured.

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: gameplay-visible behavior, data grammar, formula, ordering, or random-draw sequence. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, undefined behavior, uninitialized reads, stale state, presentation-only transients, or a lookup that reads the wrong table in an obviously unintended way. A clean-room engine may apply the stated safe rule. It should keep every other observable behavior. An engine that wants bit-exact parity may offer a compatibility switch that keeps the reference behavior.

Byte-layout tables are tagged once per table. Their rows inherit that tag.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Numeric Conventions and Overall File Layout

All file offsets are zero-based byte offsets. **Every multi-byte number in the file is big-endian**, including instruction words, immediate payloads, table sizes, per-string lengths, procedure fields, and sentinel words. Integer values are 32-bit two's-complement; floating-point payloads are IEEE-754 binary32. Flags are 32-bit bitfields. Valid offsets, counts, and lengths must be nonnegative except for the explicitly defined `FFFFFFFF` sentinels. `[Must-match]`

The file has no textual signature, version word, relocation directory, or separate native-code section. Execution starts at offset zero. The first 42 bytes are executable bootstrap/return-trampoline instructions, not a conventional passive file header. `[Must-match]`

Let N be the procedure count, I the identifier-table origin, A the identifier payload length, and T the literal-table origin:

`I = 46 + 24 x N`

`T = I + 4 + A + 4`

The following file-layout table is `[Must-match]`.

| Absolute file offset | Bytes | Interpretation |
| --- | ---: | --- |
| `0x0000` | 42 | Bootstrap and return trampolines |
| `0x002A` | 4 | Procedure count N, signed 32-bit, big-endian |
| `0x002E` | 24 x N | Procedure records |
| I | 4 | Identifier payload length A, signed 32-bit, big-endian |
| I + 4 | A | Identifier records |
| I + 4 + A | 4 | Identifier-table terminator, literal bytes `FF FF FF FF` |
| T | 4 | Literal payload length, or `FF FF FF FF` for an absent literal table |
| T + 4 | Variable | Literal records if present; otherwise initial executable stream |
| After literal records and their terminator | Variable | Global initialization and procedure instruction streams |

For a present literal table with payload length B, its payload occupies B bytes beginning at `T + 4`, followed by a four-byte `FFFFFFFF` terminator. The initial executable stream then begins at `T + 8 + B`. For an absent table, the initial executable stream begins at `T + 4`; there is no additional terminator after the absent-table word. `[Must-match]`

The loader fixes the procedure-count location at 42 and obtains I from N. It obtains its literal-reference base by adding A and four bytes to I: this lands on the **identifier terminator**, four bytes before T. Literal resolution adds a further four bytes before applying the supplied string offset, so the effective literal-reference origin is T. `[Must-match]` The loader does not scan or validate either set of string records, the sentinels, or the literal payload length. `[May-improve]`: validate the record extents, both sentinels, and the literal payload length, and reject an inconsistent file with a diagnostic. The leading bootstrap jump actually controls execution into the later code stream. `[Must-match]`

Procedure entry addresses, jump targets, condition addresses, and return addresses are **absolute offsets from byte zero of the whole file**, not offsets from the later code stream. Procedure indices and string offsets belong to different namespaces and must not be substituted for code addresses. `[Must-match]`

### 2.2 Bootstrap and Return-Trampoline Area

The normal compiler layout below is present in all 952 observed scripts; the initial-code destination varies by file. These bytes are an execution convention, not a signature checked by the loader. The table below is `[Must-match]`; host calls depend on the trampoline offsets 18, 20, 24, 28, 32, and 36.

| File offset | Bytes | Meaning |
| --- | --- | --- |
| `0x00` | `80 02` | Enter critical execution |
| `0x02` | `C0 01 00 00 00 12` | Push integer return address 18 |
| `0x08` | `80 0D` | Move that value to the return stack |
| `0x0A` | `C0 01` followed by four big-endian bytes | Push the absolute initial-code destination |
| `0x10` | `80 04` | Jump to that destination |
| `0x12` | `80 10` | End execution when the root call returns |
| `0x14` | `80 1A 80 20` | Discard a result, restore context, and return |
| `0x18` | `80 1A 80 21` | Discard a result, restore context, and return with a yield |
| `0x1C` | `80 1A 80 22` | Discard a result and complete an external-context return |
| `0x20` | `80 1A 80 23` | Discard a result and complete an external-context return with a yield |
| `0x24` | `80 24` | Transfer a return value to an external caller |
| `0x26` | `80 25` | Preserve a return value across context restoration and yield |
| `0x28` | `80 26` | Preserve a value in the current program across external-context restoration and yield |

The procedure count immediately follows the final two-byte instruction, at `0x2A`. The bootstrap first jumps **over the metadata tables**. Later global-initialization instructions establish globals, push initial values and a root argument count, leave critical execution, and jump into the root body. This later initialization is executable bytecode of variable size, not another fixed-layout header. `[Must-match]`

### 2.3 Procedure Records and Flags

Procedure record P begins at `46 + 24 x P`, for indices zero through N minus one. The record and flag tables below are `[Must-match]`.

| Record-relative offset | Bytes | Type and meaning |
| --- | ---: | --- |
| `+0x00` | 4 | Signed 32-bit, big-endian: procedure-name offset relative to I |
| `+0x04` | 4 | 32-bit big-endian flag bitfield |
| `+0x08` | 4 | 32-bit big-endian scheduled deadline in milliseconds; compared as unsigned when firing |
| `+0x0C` | 4 | Signed 32-bit, big-endian: absolute condition-code address |
| `+0x10` | 4 | Signed 32-bit, big-endian: absolute body entry address |
| `+0x14` | 4 | Signed 32-bit, big-endian: declared argument count |

| Mask | Procedure property | Observable interpretation |
| --- | --- | --- |
| `0x00000001` | Timed | Check the deadline and arrange a procedure call when it has passed |
| `0x00000002` | Conditional | Execute the condition stream and use its returned payload as a predicate |
| `0x00000004` | Import | Resolve a procedure in the shared export registry by this record's name |
| `0x00000008` | Export | Declared export metadata; merely setting this bit does not register a callable symbol |
| `0x00000010` | Critical | Normal VM calls and scheduled calls can enter critical execution on entry |
| No verified mask | Pure | Not defined, tested, or enforced by this reference |

The bits can be combined; import and critical are independently tested on relevant call paths. Other bits, including `0x20`, have no verified procedure meaning. Runtime program-state bits are a different bitfield and must not be interpreted as procedure attributes. `[Must-match]`

Index zero is conventionally the root/main record. String-based VM procedure lookup deliberately skips it; host name lookup scans from zero. `[Must-match]` Multiple procedure records may share one body address. Declared argument counts are metadata used by explicit checks and external resolution; an ordinary local call does not automatically compare them with a supplied count. `[Must-match]`

Scheduling mutates the loaded procedure-table bytes, retaining the big-endian representation. `[Must-match]` The available corpus has 17321 records, all initially with flags zero and argument count zero; it therefore does not independently demonstrate imported, critical, or argument-taking calls.

### 2.4 Identifier and Literal String Records

Both table payloads use the following record form. The record table and the offset rules below are `[Must-match]`.

| Record-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `+0` | 2 | Unsigned 16-bit, big-endian: stored byte length S |
| `+2` | S | NUL-terminated byte string, with padding to the stored length |

Observed stored lengths are even and include the terminating zero and any final alignment byte. For a string containing K nonzero bytes, the ordinary stored length is `2 x ceil((K + 1) / 2)`, and its record occupies `2 + S` bytes. Text interpretation stops at the first zero; padding is not text. The table's four-byte payload length counts record headers and padding, but excludes its own length word and the final sentinel. `[Must-match]`

Name references address the **first text byte**, relative to I, not the two-byte record header. The first name thus normally has offset 6. Identifier records contain procedure and external-variable names; there is no separate procedure-name encoding. `[Must-match]`

Literal references likewise address the first text byte, but relative to T, including that table's four-byte length prefix in the offset. A present table's first literal normally has offset 6. A static-string value with payload Q resolves to byte `T + Q`. It is not an ordinal string number and is not relative to the identifier table or code stream. `[Must-match]`

An empty literal table uses signed value -1, bytes `FF FF FF FF`, in place of a payload length. `[Must-match]` The corpus contains 828 absent literal tables and 124 present ones, containing 321 literal records in total. A zero-length present table is not observed; the loader itself does not interpret the literal length and therefore does not establish validation rules for that alternative. `[May-improve]`: accept a zero-length present table followed by its `FFFFFFFF` terminator, and reject any other inconsistent length with a diagnostic.

Names and literals are raw single-byte strings, without a code-page declaration, Unicode decoding, or escape processing at load time. Procedure and export-name lookup is case-insensitive for ordinary ASCII names. String-value comparisons are instead case-sensitive bytewise comparisons. `[Must-match]`

### 2.5 Instruction Words, Immediates, and Tagged Stack Records

Instruction words are unsigned 16-bit, **big-endian**. Most core instructions occupy two bytes and obtain operands from a stack. Constant-push instructions occupy six bytes. The instruction-layout and tag tables in this section are `[Must-match]`.

| Instruction-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `+0` | 2 | Big-endian instruction word, also used as the pushed value's tag |
| `+2` | 4 | Big-endian payload; signed integer bits, binary32 bits, or a reference offset |

| Word/tag | Value interpretation |
| --- | --- |
| `0xC001` | Signed 32-bit integer |
| `0xA001` | IEEE-754 binary32 floating-point value |
| `0x9001` | Static string reference, offset from T |
| `0x9801` | Dynamic string reference in the current program's runtime string storage |
| `0x8001` | Generic immediate push; records tag `0x8001`, not the normal integer tag |

The dispatch slot is the instruction word's low ten bits, mask `0x03FF`. All five words above therefore select slot 1 and consume a four-byte immediate. Bit 15 must be set; this is checked before dispatch. The instruction tag is preserved in full, so `0x8001` must not be normalized to `0xC001`. Ignoring tag bits when decoding would also misclassify typed pushes as host extensions. `[Must-match]`

Decoding fetches the word W and advances the instruction address by two. If bit 15 of W is clear, a bad-opcode VM error is raised. Otherwise bits 10-14 (`0x7C00`) are **ignored for dispatch in every slot**: for example `0xC000` executes as `0x8000` and `0xC039` as `0x8039`. `[Must-match]` Slot 1 reads the four-byte payload and pushes it with the full word W as its tag, without normalizing it. Exactly 32 words are therefore constant pushes, `0x8001 + n x 0x0400` for n = 0 through 31 (`0x8001`, `0x8401`, `0x8801`, ..., `0xFC01`). Each is a valid six-byte instruction and raises no error when executed; in particular `0xC801`, `0xA801` and `0x8401` are valid pushes. `[Must-match]` Only the five words in the table above have defined value semantics, and a compiler emits only those. The other 27 leave a value with a non-standard tag, which later instructions treat as follows:

- Operand checks that first clear the dynamic-string bit `0x0800` (mask `0xF7FF`) accept `0xC801` as an integer and `0xA801` as a float, and reject `0x8401` and every other non-standard tag with a VM type error. `[Must-match]`
- Checks that require the exact integer tag `0xC001` reject every non-standard tag with a VM error. `[Must-match]`
- The section 3.5 arithmetic, comparison and logical instructions classify only the exact tags `0xC001`, `0xA001`, `0x9001` and `0x9801`. With any other operand tag the reference result is undefined: no error is raised, and either no result or an uninitialized one is pushed. `[May-improve]`: raise a VM type error.
- String resolution tests bit `0x0800` before bit `0x1000`, so a tag with `0x0800` set that reaches it unchecked is read as a dynamic-string reference. `[May-improve]`: resolve strings only for the exact tags `0x9001` and `0x9801`.
- Conditional branches test only the payload, so the tag does not matter there. `[Must-match]`

`[May-improve]`: a loader may reject, or execution may raise the undefined-instruction VM error for, any word with bits 10-14 set other than `0xC001`, `0xA001`, `0x9001` and `0x9801`, because well-formed compiler output never contains them.

Each complete value on either runtime stack occupies six bytes:

| Stack-slot-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `+0` | 4 | Payload, big-endian |
| `+4` | 2 | Tag, big-endian |

This is **payload before tag**, whereas a constant instruction is **tag before payload**. Object references, program references, wait-predicate references, and return addresses use the integer tag `0xC001` with a 32-bit payload; there is no distinct object-reference tag in this VM. Object payloads are opaque process-local references, with zero representing null, not portable object IDs serialized in the script file. `[Must-match]`

## 3. Behavioral Rules & Edge Cases

### 3.1 Execution State, Two Stacks, and Variable Indexing

Each program has a separate value stack and return stack, **2048 bytes each**. They grow toward increasing offsets. Both begin empty; the instruction address begins at zero; the local-frame and global-area bases initially equal -1. Complete tagged pushes use six bytes, and the push guards reject an operation that would end at or beyond byte 2048. Thus at most 341 complete tagged values, occupying 2046 bytes, fit on an otherwise empty stack. Return addresses and saved frame information compete for return-stack space. `[Must-match]`

The value stack holds expression operands, arguments, locals, globals, saved execution context, and results. The return stack is not restricted to addresses: ordinary transfers can place any tagged value there. No implicit conversion occurs when duplicating, swapping, transferring, fetching, or storing a value; both payload and tag are copied. `[Must-match]`

Let F be the current local-frame base, G the global-area base, and J a zero-based variable index. These bases are **byte offsets into the value stack**:

- A local slot begins at `F + 6 x J`; a global slot begins at `G + 6 x J`.
- A fetch copies the four-byte payload and following two-byte tag from the chosen slot onto the value stack.
- A store consumes the index above the value, then overwrites the chosen slot with the consumed value; it does not push a result or allocate a slot.
- Establishing the global area records the current value-stack byte height. Subsequent initializer pushes become globals at that position.
- Establishing a local frame consumes an argument-count value, saves the previous F as an integer on the return stack, and sets F to the remaining value-stack byte height minus six times that count. Existing argument slots become the first local indices; further pushed local values follow them.

The five rules above are `[Must-match]`.

Globals here are **per-program stack variables**, not the game's persistent global, map, or script-local variables accessed by host extensions. `[Must-match]` Variable instructions do not establish bounds from a declared local/global count. Negative indices, unset bases, references past the live stack, or corrupted tags can access invalid or unintended memory. `[May-improve]`: raise a VM error when a computed slot does not lie wholly within the live value stack, or when its base is still -1.

The local-fetch instruction requires an exact integer index tag. Frame-base restoration requires an integer tag. `[Must-match]` Global fetch/store and local store do not consistently validate index tags. Frame establishment consumes the argument-count payload without checking its tag. `[May-improve]`: use the payload as the reference does for integer-tagged operands, and raise a VM type error for any other tag. Truncation back to F repeatedly pops complete values until byte heights are equal. `[Must-match]` An unreachable or misaligned F can underflow. `[May-improve]`: raise a VM error when F is above the current height or not reachable in six-byte steps.

Runtime strings are program-owned, NUL-terminated byte strings. Static and dynamic references use different storage origins, even when their offsets match. Clearing qualifier bit `0x0800` from tag `0x9801` yields `0x9001`, allowing operations that accept either string kind. Dynamic strings are allocated during execution, can reuse an identical stored string, and are not an extra serialized table in a fresh `.INT`. Cross-program string transfer must copy characters into the receiving program rather than reuse another program's offset. `[Must-match]`

### 3.2 Core Stack, Variable, and Metadata Instructions

Stack notation below lists the top at the **right**. A and B denote complete tagged values; J denotes an integer index when required. An omitted stack is unchanged. `A, B -> B, A` is a mathematical stack transition, not source syntax. Each table row in this section is `[Must-match]` except where the text below tags a row differently.

| Word | Operation | Stack transition and effects |
| --- | --- | --- |
| `0x8000` | No operation | No stack or address effect beyond consuming the instruction |
| `0x8001` | Generic immediate push | Push the following four-byte payload with the full fetched word as tag; typed variants are in Section 2.5 |
| `0x800C` | Return-to-value transfer | Return stack loses A; value stack gains A |
| `0x800D` | Value-to-return transfer | Value stack loses A; return stack gains A |
| `0x8012` | Fetch global | `J -> value at G + 6 x J` |
| `0x8013` | Store global | `A, J -> nothing`; replace global slot J with A |
| `0x8014` | Fetch external variable | `name offset -> registered value`; offset addresses identifier text relative to I |
| `0x8015` | Store external variable | `A, name offset -> nothing`; replace the registered value, copying string content when necessary |
| `0x8016` | Export variable | `name offset -> nothing`; register an integer initialized to zero; conflicting ownership is an error |
| `0x8017` | Export procedure | `argument count, procedure index -> nothing`; register the name, body address, and supplied argument count |
| `0x8018` | Swap value-stack top | `A, B -> B, A` |
| `0x8019` | Swap return-stack top | Same exchange, but on the return stack |
| `0x801A` | Discard value | `A -> nothing` |
| `0x801B` | Duplicate value | `A -> A, A` |
| `0x801E` | Discard return-stack value | Return stack loses one complete tagged value |
| `0x8027` | Check declared argument count | `procedure index, expected count -> nothing`; error unless the record's argument count equals the expected count |
| `0x8028` | Find procedure by string value | `string reference -> integer procedure index`; case-insensitive search of indices 1 through N minus one; absent name is an error |
| `0x8029` | Restore local-frame base | Pop an integer from the return stack into F |
| `0x802A` | Discard through current frame | Pop value-stack records until its byte height equals F |
| `0x802B` | Establish local frame | Consume the argument count, save old F on the return stack, and set F as described in Section 3.1 |
| `0x802C` | Establish global area | Set G to the current value-stack byte height; consume nothing |
| `0x802D` | Fetch procedure body address | `procedure index -> integer absolute body address`; requires an exact integer tag |
| `0x802E` | Counted discard, recovered quirk | Consume an integer count, then use the changing-limit behavior below; not reliably a fixed-count discard (see the `[May-improve]` rule below) |
| `0x8031` | Store local | `A, J -> nothing`; replace local slot J with A |
| `0x8032` | Fetch local | `J -> value at F + 6 x J`; requires an exact integer tag |

For `0x802E`, let C initially be the consumed count and let K begin at zero. While K is less than C, pop another tagged value, replace C with that popped value's **payload**, and increment K. The loop therefore uses a newly popped payload as its next bound. An initial nonpositive count discards no further values. `[Must-match]` for the count's integer-tag check and the nonpositive case. The changing bound comes from the loop counter being overwritten by each popped payload. The reference's own annotations say the original loop differs, so this is stale state, not an intended feature. `[May-improve]`: after the count, discard exactly C further complete values. Document which rule an engine uses; Section 4.2 gives the result under both.

Exported procedure and variable names share case-insensitive registries across programs, but occupy separate namespaces. Registration supports names of at most 31 retained bytes; longer identifiers are truncated in registry storage. Export registration is an executed operation, not a loader action triggered by a record flag. Missing imported procedures and external variables are errors. Strings in external variables are stored as character content and are copied into the fetching program's dynamic storage. `[Must-match]`

### 3.3 Addressing, Branching, Scheduling, and Program Control

Unless otherwise stated, these operations obtain every operand from the value stack; there is **no inline branch displacement** after the instruction word. Each table row is `[Must-match]`.

| Word | Operation | Stack transition and effects |
| --- | --- | --- |
| `0x8002` | Enter critical execution | Set critical runtime state |
| `0x8003` | Leave critical execution | Clear critical runtime state |
| `0x8004` | Unconditional jump | `address -> nothing`; set the instruction address to the absolute payload; requires an integer after ignoring qualifier bit `0x0800` |
| `0x8005` | Call procedure | Consume procedure index; a local call jumps to its body without automatically saving a continuation, establishing a frame, or consuming an argument count |
| `0x8006` | Schedule timed procedure | `delay in seconds, procedure index -> nothing`; set deadline to current milliseconds plus 1000 times delay and set the timed bit |
| `0x8007` | Schedule conditional procedure | `condition address, procedure index -> nothing`; store the absolute condition address and set the conditional bit |
| `0x8008` | Start attached child | `filename string -> nothing`; start a fresh script program and block this parent while attached |
| `0x8009` | Replace program | `filename string -> nothing`; start a fresh replacement, transfer its parent association, and terminate the old program |
| `0x800A` | Spawn attached child | `filename string -> nothing`; start a fresh child and use the distinct spawn-blocked state; inherit critical execution when applicable |
| `0x800B` | Start independent program | `filename string -> nothing`; start a fresh program without a parent link; this does not clone the current stacks |
| `0x800E` | Terminate and purge | Set ended state, release program references, and unblock a spawn-waiting parent |
| `0x800F` | Detach | Clear the parent's attached/spawn-blocked states and its child link; no value is consumed |
| `0x8010` | Mark execution ended | Set ended state without the explicit purge operation |
| `0x8011` | Stop program | Set stopped state; ordinary scheduling does not resume it automatically |
| `0x802F` | Conditional branch | `target, condition -> nothing`; jump to target when the condition payload is zero, otherwise continue |
| `0x8030` | Loop condition | `target, condition -> target` when nonzero; when zero, consume both and jump to target |
| `0x8047` | Wait | `milliseconds -> nothing`; record a wait interval and install the recovered readiness predicate described below |
| `0x8048` | Cancel one procedure | `procedure index -> nothing`; zero its entire flags word, deadline, and condition address |
| `0x8049` | Cancel every procedure | Apply the same three-word clearing to every procedure record |
| `0x804A` | Enter critical execution | Alias of `0x8002` |
| `0x804B` | Leave critical execution | Alias of `0x8003` |

Branch predicates inspect the **raw 32-bit payload**, not its tag or decoded numeric/string value. Float negative zero therefore branches as nonzero, and a string reference's branch truth depends on its offset rather than its text. `0x802F` always consumes the target, but the true path of `0x8030` leaves it on the stack for loop bookkeeping. Neither performs a branch-target type check. `[Must-match]`

A local `0x8005` enters critical state when the selected procedure has bit `0x10`; it does not automatically leave that state on return. Timed and conditional call preparation also tests critical metadata. The synchronous host lifecycle-call path does not independently set critical state from that bit, so critical is not a uniform promise across every entry route. `[Must-match]`

Timed events fire when the unsigned deadline is **strictly less than** current milliseconds, not at equality. Conditional scheduling takes precedence if both scheduling bits are set. A condition stream must leave a tagged predicate result; a nonzero payload schedules its procedure. Firing clears the **whole procedure flag word**, not only the triggering scheduling bit. Canceling likewise removes import/export/critical bits along with scheduling metadata. These operations affect the loaded image, not the disk file. `[Must-match]`

The wait operation records current milliseconds and current milliseconds plus the supplied delay. However, the recovered readiness predicate succeeds while current time is **less than or equal to the deadline**, and the interpreter clears waiting on predicate success. A positive wait can thus continue immediately; a wait first checked after its deadline keeps waiting indefinitely, because the predicate never succeeds again. This is an inverted comparison, not a conventional sleep guarantee, and it still needs original-executable verification. `[Must-match]` for the usual outcome: a wait checked before its deadline releases on that first check. `[May-improve]`: release the wait on its first readiness check whatever the current time, so a late first check cannot hang the program.

Programs normally run for an instruction budget per update; the default budget is 10. `[Must-match]` Critical execution continues beyond that budget until critical state is cleared or a stopped/ended/blocked/yield condition interrupts it. Critical state is a single bit, not a nested counter. A return-with-yield ends the current interpreter invocation; its transient yield bit is then cleared. A permanently stopped or ended state is not the same as yielding. `[Must-match]`

### 3.4 Return Instructions and Saved Context

A saved value-stack execution context is three integer-tagged values in this bottom-to-top order: **status bits, wait-predicate reference, window identifier**. Restoring it consumes all three and retains only the low 16 bits of status. Return-stack addresses are complete tagged values, but address-return operations discard the tag without checking it. `[Must-match]`

For external-context returns, the relevant additional return-stack values are a caller's saved status, saved wait-predicate reference, and caller-program reference, with the caller-program reference on top after the trampoline address has been removed. Restoring these values restores the caller's wait and status state. Stack/frame cleanup must already have exposed this context in the required order. `[Must-match]`

The following table is `[Must-match]`.

| Word | Operation | Exact return/context effects |
| --- | --- | --- |
| `0x801C` | Return | Pop one return-stack tagged value and use its payload as the next instruction address |
| `0x801D` | Return and yield | Perform `0x801C`, then request a yield |
| `0x801F` | Restore current context | Consume and restore the value-stack context triple; no address is popped |
| `0x8020` | Restore and return | Restore the current context, then perform `0x801C` |
| `0x8021` | Restore, return, and yield | Restore the current context, then perform `0x801D` |
| `0x8022` | External-context return | Restore current context and caller state, then pop a return address in the current program |
| `0x8023` | External-context return and yield | Same restoration as `0x8022`, then return and yield in the current program |
| `0x8024` | External value return | Remove a result from the value stack; restore current and caller context; push the result into the caller; pop a return address in each program |
| `0x8025` | Preserve value and yield | Temporarily remove a result; restore current context; return and yield; put the result back on the current value stack |
| `0x8026` | Preserve value across external-context yield | Temporarily remove a result; restore current and caller context; return and yield in the current program; put the result back there, not in the caller |

For `0x8024`, static or dynamic string results are copied into the caller and retagged as dynamic strings; other payloads and tags are copied unchanged. If the restored caller is critical, the current program's critical state is cleared. None of these instructions by itself discards a local frame or restores its base. `[Must-match]`

### 3.5 Arithmetic, Comparisons, Logical, and Bitwise Instructions

Binary operations pop B first, then A, and compute **A operation B**. Their usual value-stack transition is `A, B -> result`. Comparison and logical results are integers 0 or 1, not specially tagged booleans. Table rows are `[Must-match]` except where the text below tags a case differently.

| Word | Operation | Result and supported numeric behavior |
| --- | --- | --- |
| `0x8033` | Equal | Integer 1 when A equals B, otherwise 0 |
| `0x8034` | Not equal | Integer 1 when A differs from B, otherwise 0 |
| `0x8035` | Less than or equal | Integer result for A less than or equal to B |
| `0x8036` | Greater than or equal | Integer result for A greater than or equal to B |
| `0x8037` | Less than | Integer result for A less than B |
| `0x8038` | Greater than | Integer result for A greater than B |
| `0x8039` | Add or concatenate | Numeric sum, or dynamic-string concatenation if either operand is a string; integer promotion exception below |
| `0x803A` | Subtract | A minus B; integer for two integers, otherwise binary32 for valid mixed numeric inputs |
| `0x803B` | Multiply | A times B; same numeric result-type rule as subtraction |
| `0x803C` | Divide | A divided by B; integer division truncates toward zero; otherwise binary32; zero divisor is an error |
| `0x803D` | Remainder | Integer remainder after division truncated toward zero; floating-point operands and zero divisor are errors |
| `0x803E` | Logical AND | Integer conjunction using the type-dependent truth rules below; no short-circuit evaluation |
| `0x803F` | Logical OR | Integer disjunction using those truth rules; no short-circuit evaluation |
| `0x8040` | Bitwise AND | Integer bitwise conjunction; recovered floating-tag conversion quirk below |
| `0x8041` | Bitwise OR | Integer bitwise inclusive disjunction; same conversion quirk |
| `0x8042` | Bitwise XOR | Integer bitwise exclusive disjunction; same conversion quirk |
| `0x8043` | Bitwise complement | `A -> integer complement of A's raw 32-bit payload`; input tag is ignored |
| `0x8044` | Convert float to integer | `A -> converted value`; binary32 truncates toward zero, not mathematical floor |
| `0x8045` | Logical NOT | `A -> integer 1 if the raw payload is zero, otherwise 0`; input tag is ignored |
| `0x8046` | Negate raw payload | `A -> integer negative of the signed 32-bit payload`; does not decode a float |

Comparisons between integers are signed comparisons. A mixed integer/float comparison converts the integer numerically to binary32; two floats use their numeric values. If either operand is a string, compare character content lexicographically, case-sensitively, through the first NUL. Convert a numeric partner to signed decimal text for an integer, or fixed-point text with exactly five fractional digits for a finite float. Nonfinite float text follows the target runtime's formatting, not a portable spelling. Do not parse a string as a number. Thus comparing integer 10 with string text `2` uses lexical ordering, not a numeric comparison. Ordinary IEEE unordered behavior applies to floating NaN comparisons. `[Must-match]` (the nonfinite text spelling is `[May-improve]`: format NaN as `nan` and infinities as `inf` or `-inf`).

Addition concatenates the text of A followed by the text of B when either is a string, using the same decimal/five-fractional-digit formatting for a numeric partner. Its result is a dynamic string. Otherwise mixed numeric addition produces binary32. For two integers, let M be 2147483647 and m be -2147483648. An integer result is retained exactly when both conditions hold:

- B is nonpositive, or `A < M - B`.
- B is nonnegative, or `A >= m - B`.

Otherwise the operands are converted to binary32 and added as floats. The strict positive-side inequality means an exactly representable integer sum equal to M can still promote: `0 + M` produces a float, while `M + 0` remains an integer. `[Must-match]` Subtraction, multiplication, and raw negation have no comparable overflow promotion. `[Must-match]` Their signed-overflow results are not established as a portable wraparound contract. Integer division of m by -1 is likewise unsafe rather than a guaranteed result. `[May-improve]`: wrap every integer subtraction, multiplication, negation, and division result modulo 2^32, so m divided by -1 yields m.

Logical AND/OR treat a string as true even if its text is empty or its offset is zero. Integers are true when their payload is nonzero. Numeric floats are normally tested after clearing the sign bit, so both positive and negative zero are false. One recovered AND exception uses the entire float payload when A is a float and B is a string; negative zero is then true. Logical NOT and branch instructions do **not** use these type-dependent rules: they test the raw payload. There is no single truth conversion shared by all VM instructions. `[Must-match]`

For binary bitwise instructions, integer payloads operate directly. A float-tagged operand is handled by numerically converting its **signed payload bits as an integer** to binary32 and back to an integer, not by decoding its original float value. This can round low payload bits. `[Must-match]` The conversion back to an integer is out of range when rounding reaches 2^31. `[May-improve]`: produce `80 00 00 00` (the x86 integer-indefinite value) for any out-of-range or NaN float-to-integer conversion. A string in the right position can supply its raw offset as an integer operand when the left operand is numeric. `[Must-match]` A string in the left operand position consumes both operands and pushes no result, unbalancing the stack. `[May-improve]`: raise a VM type error.

The float-to-integer conversion rejects an exact static-string tag `0x9001`. Integers pass through unchanged. `[Must-match]` The recovered path passes a dynamic-string tag `0x9801` through unchanged. `[May-improve]`: raise the same VM error for both string tags. Float conversion outside signed 32-bit range, and NaN/infinity conversion, have no portable defined integer result. `[May-improve]`: produce `80 00 00 00`, as for the bitwise conversion above.

String operands outside addition/comparison do not receive uniform validation. When the left operand of subtraction, multiplication, or division is numeric, a non-float right operand can be treated as its raw integer payload. For remainder, a non-float right tag can likewise provide a raw divisor. `[Must-match]` Subtraction, multiplication, and division with a string on the left consume the inputs without producing a result. Remainder also does not parse strings: a nonnumeric left tag yields no result. Unsupported tags must not be assumed to produce a consistent type error. `[May-improve]`: raise a VM type error whenever an operation would otherwise consume its operands and push nothing. No core shift, exponentiation, array, or dedicated object-arithmetic instruction is present in this range.

### 3.6 Call-Frame Lifecycle

A normal local call is a bytecode-managed protocol rather than one atomic call instruction. The steps below are `[Must-match]`:

1. The caller pushes an integer continuation address and transfers it to the return stack with `0x800D`.
2. The caller pushes argument values, their count, and the procedure index on the value stack.
3. `0x8005` removes only the index and enters the selected local body; the argument count remains on top.
4. The callee executes `0x802B`, consuming the count, saving the previous local base, and treating argument zero as local zero.
5. Expression and local storage operations run above the remaining caller stack and globals.
6. Before returning, generated bytecode preserves any result while `0x802A` removes arguments and locals, restores the previous local base with `0x8029`, and returns through `0x801C`.

There is no automatic result slot, implicit argument copying, or automatic argument-count validation for a local call. A valid result-preserving epilogue can move its result onto the return stack, truncate locals, move the result back, restore the base, and return. The complete fixture uses exactly that sequence. Other compiler-generated cleanup sequences can implement the same contract. `[Must-match]`

For a host-initiated local procedure call, preparation saves the old instruction address and a trampoline address on the return stack, then pushes the saved value-stack context triple and integer argument count zero. The trampoline is decimal offset 20 for a deferred/scheduled call or decimal offset 24 for synchronous execution. The procedure's cleanup must leave a result or placeholder above the saved context: the trampoline explicitly discards it before restoring context. Host calls therefore use more state than an ordinary local `0x8005`. `[Must-match]`

Host-initiated external calls use decimal trampoline offsets 28 or 32. The callee return stack receives its old instruction address, the caller's status, the caller's wait predicate, the caller-program reference, and the trampoline address, in that order. The callee value stack receives its own saved context and zero arguments. The caller is blocked while this context is active. The return trampolines restore both programs' relevant state; they are not native machine return addresses. `[Must-match]`

An imported `0x8005` instead attempts to use the shared export registry, consume an explicit argument count, transfer arguments into the other program, and use trampoline 36 for a value-returning external call. **This recovered path is not a reliable executable argument-transfer specification:** it uses temporary stack storage without allocating that storage, and narrows argument payloads to 16 bits. The first loop moves exactly the supplied count into temporary storage, but the second loop, which moves them into the callee, runs one extra time, so it pops an empty temporary stack even for zero arguments. Its argument-count mismatch path also prepares a diagnostic without raising the normal VM error. Do not infer working imported calls from the record format. `[May-improve]`: raise a VM error on an argument-count mismatch, and either transfer exactly the declared number of complete 32-bit tagged values in their original order or reject imported calls with a VM error. Document which choice an engine makes. The separate zero-argument host external-call preparation does not use those faulty transfer loops. `[Must-match]`

### 3.7 Extension Boundary and Requested Lifecycle Entries

All words `0x8000` through `0x804B` are registered by the core VM. The general library occupies registrations beginning at `0x804C`, through the library range ending at `0x80A0`; game-specific registrations begin at `0x80A1` and extend through `0x8155` in this revision. This describes registration ranges, not a promise that every possible word in an arbitrary higher range is valid. No full host-call list is included here. `[Must-match]`

The following are **public procedure names**, located through case-insensitive identifier lookup. Event numbers are host dispatch categories; neither the event number nor the procedure's eventual table index is an opcode. The table below is `[Must-match]`.

| Public entry name | Host event number, decimal | Observable entry purpose |
| --- | ---: | --- |
| `start` | 1 | Script startup/initialization |
| `map_enter_p_proc` | 15 | Map-entry notification |
| `map_update_p_proc` | 23 | Map-update notification |
| `talk_p_proc` | 11 | Dialogue/talk request |
| `look_at_p_proc` | 21 | Short object look-at request |
| `description_p_proc` | 3 | Detailed object-description request |

The names do not have fixed procedure-table indices. A newly loaded program first executes its bootstrap and global initialization; later host invocations use cached procedure indices and synchronous call preparation. Missing names are cached as zero. The generic host invocation path substitutes **procedure index 1** for that zero, whereas map-entry and map-update broadcasts first test for a present procedure. A missing callback is therefore not uniformly a silent no-op. Do not confuse the fallback index with the event number or assume that it proves the existence of a procedure named `start`. `[Must-match]`

### 3.8 Error and Validation Boundaries

Stack underflow, detected stack overflow, invalid explicitly checked operand types, division by zero, absent symbols, and an unregistered **in-range** dispatch slot raise VM errors. The active execution is unwound and marked ended/error. `[Must-match]` The ordinary runtime state bits include ended `0x0001`, error `0x0004`, stopped `0x0008`, waiting `0x0010`, attached-call blocking `0x0020`, transient yield `0x0040`, critical execution `0x0080`, and spawn blocking `0x0100`. They are saved as context integers, not procedure-table flags. `[Must-match]`

The dispatch array has 342 entries, indexed zero through 341. Execution masks an instruction to ten bits but does not independently bound that index before reading the array. A word with bit 15 clear is a bad-opcode error. `[Must-match]` An index from 342 through 1023 can instead read outside the dispatch array. `[May-improve]`: treat any index from 342 through 1023 as an unregistered slot and raise the undefined-instruction VM error. Section 2.5 defines the 32 constant-push words and how non-standard push tags behave; other instruction words with bits 10-14 set simply dispatch on their low ten bits.

The file loader does not verify a complete read, minimum file length, all table extents, argument counts, NUL termination, string-reference bounds, or code destinations. Instructions likewise do not universally check image boundaries, alignment, variable bounds, or procedure indices. A malformed file can access invalid memory rather than report a clean format error. `[May-improve]`: apply the safe-parser rules in the next sentence and reject the file, or raise a VM error, with a diagnostic. There is no checksum. `[Must-match]` A safe independent parser should validate table arithmetic without overflow, bound every record and reference, check sentinel/length consistency, restrict execution to complete instructions, and separately enforce stack/slot limits.

No purity flag prevents side effects during conditional evaluation. `[Must-match]` Scheduling and external-context code also contain recovered context/transfer defects, so static layouts and intended context shapes must be distinguished from guarantees of safe execution in those paths. The synthetic execution fixture deliberately avoids imported calls and timed/conditional scheduling. `[May-improve]` for those recovered defects, under the safe rules given in Sections 3.1 to 3.6.

## 4. Synthetic Test Fixtures (Hex / Text)

### 4.1 Complete `.INT`: Two Procedures, Strings, a Call, and a Branch

This is a complete synthetic file of `0x011A` (282) bytes. Each line begins with a hexadecimal file offset followed by the actual bytes. No bytes, zero-fill ranges, or external assets are omitted.

```text
0000: 80 02 C0 01 00 00 00 12 80 0D C0 01 00 00 00 84
0010: 80 04 80 10 80 1A 80 20 80 1A 80 21 80 1A 80 22
0020: 80 1A 80 23 80 24 80 25 80 26 00 00 00 02 00 00
0030: 00 06 00 00 00 00 00 00 00 00 00 00 00 00 00 00
0040: 00 9C 00 00 00 00 00 00 00 0E 00 00 00 00 00 00
0050: 00 00 00 00 00 00 00 00 00 FC 00 00 00 02 00 00
0060: 00 10 00 06 6D 61 69 6E 00 00 00 06 73 75 6D 78
0070: 79 00 FF FF FF FF 00 00 00 06 00 04 68 69 00 00
0080: FF FF FF FF 80 2C C0 01 00 00 00 00 C0 01 00 00
0090: 00 00 80 03 C0 01 00 00 00 9C 80 04 80 2B C0 01
00A0: 00 00 00 C0 80 0D C0 01 00 00 00 05 C0 01 00 00
00B0: 00 07 C0 01 00 00 00 02 C0 01 00 00 00 01 80 05
00C0: C0 01 00 00 00 E8 80 18 C0 01 00 00 00 0A 80 38
00D0: 80 2F C0 01 00 00 00 63 C0 01 00 00 00 00 80 13
00E0: C0 01 00 00 00 F6 80 04 C0 01 FF FF FF FF C0 01
00F0: 00 00 00 00 80 13 80 2A 80 29 80 1C 80 2B C0 01
0100: 00 00 00 00 80 32 C0 01 00 00 00 01 80 32 80 39
0110: 80 0D 80 2A 80 0C 80 29 80 1C
```

Expected metadata:

| Item | Expected value |
| --- | --- |
| Procedure count at `0x002A` | 2 |
| Record 0 at `0x002E` | Name offset 6; flags 0; deadline 0; condition address 0; body `0x009C`; argument count 0 |
| Record 1 at `0x0046` | Name offset 14; flags 0; deadline 0; condition address 0; body `0x00FC`; argument count 2 |
| Identifier origin I | `0x005E`; payload length 16 |
| Identifier text at I + 6 | `main`, followed by two zero bytes within its stored length 6 |
| Identifier text at I + 14 | `sumxy`, followed by one zero byte within its stored length 6 |
| Identifier terminator | `0x0072` |
| Literal origin T | `0x0076`; payload length 6 |
| Literal text at T + 6 | `hi`, followed by two zero bytes within its stored length 4 |
| Literal terminator | `0x0080` |
| Initial executable stream | `0x0084` |

Execution begins at zero with empty stacks and both bases -1. Advance repeatedly until the ended state; an instruction-budget boundary is not final termination.

1. The bootstrap places return address 18 on the return stack and jumps to `0x0084`.
2. Initialization establishes G as zero, creates global slot zero with integer value zero, pushes a root argument count of zero, leaves critical state, and jumps to `0x009C`.
3. The root establishes F as 6. It saves continuation `0x00C0`, pushes integer arguments 5 and 7, count 2, and procedure index 1, then calls at `0x00BE`.
4. The helper at `0x00FC` establishes its frame, fetches locals zero and one, and adds them at `0x010E`, producing integer 12.
5. The helper epilogue transfers the result to the return stack, discards both argument slots, transfers the result back, restores F, and returns to `0x00C0`.
6. The root arranges target `0x00E8` below the comparison result for `12 greater than 10`. The conditional branch at `0x00D0` consumes both target and true predicate, so execution falls through.
7. The true arm stores integer 99 in global slot zero and jumps to the join at `0x00F6`. The alternative arm would store -1.
8. Root cleanup restores F to -1 and returns to offset 18. The ended instruction there leaves the next instruction address at decimal 20.

Final state: G is zero; F is -1; the return stack is empty; the value stack contains only global slot zero, bytes `00 00 00 63 C0 01`; no host extension has executed.

Branch mutation: replace the four-byte immediate at `0x00AE` with `00 00 00 01`. The helper then computes 6; the same comparison is false, and the branch goes to `0x00E8`. Final global-slot bytes become `FF FF FF FF C0 01`. Both variants must finish with an empty return stack and the same global/local bases.

Argument-metadata mutation: change record 1's declared argument-count word at `0x005A` from 2 to 3, without adding a check instruction. The ordinary local call still uses the explicitly supplied count 2 and gives the original result; `0x8005` does not check that declaration.

### 4.2 Isolated Typed-Value and Arithmetic Fragments

These are instruction fragments, **not complete `.INT` files**. Run each independently with empty operand stacks, stop at the end of the listed bytes, and do not fall through into unspecified memory. For the static-string fragment, use T and the literal bytes from Section 4.1. An expected tag/payload pair is written separately from the instruction stream; actual stack bytes place the payload first.

| Purpose | Complete fragment bytes, hexadecimal | Expected value-stack result |
| --- | --- | --- |
| Integer operand order | `C0 01 00 00 00 07 C0 01 00 00 00 03 80 3A` | Integer 4, stack bytes `00 00 00 04 C0 01` |
| Negative integer division | `C0 01 FF FF FF F9 C0 01 00 00 00 03 80 3C` | Integer -2, payload `FF FF FF FE`, tag `C0 01` |
| Float conversion truncates toward zero | `A0 01 BF E0 00 00 80 44` | Integer -1; the input is binary32 -1.75 |
| Static string reference | `90 01 00 00 00 06` | Tag `90 01`, offset 6, resolving to synthetic text `hi` |
| Raw-payload NOT of negative float zero | `A0 01 80 00 00 00 80 45` | Integer 0, not 1 |
| Raw-payload negation of binary32 1.5 | `A0 01 3F C0 00 00 80 46` | Tag `C0 01`, payload `C0 40 00 00`, not a float result |
| Positive-limit addition promotion | `C0 01 00 00 00 00 C0 01 7F FF FF FF 80 39` | Tag `A0 01`, payload `4F 00 00 00`; binary32 rounding gives 2147483648 |
| Reversed addition retains integer | `C0 01 7F FF FF FF C0 01 00 00 00 00 80 39` | Tag `C0 01`, payload `7F FF FF FF` |
| Floating-tag bitwise conversion quirk | `A0 01 3F 80 00 01 C0 01 FF FF FF FF 80 40` | Tag `C0 01`, payload `3F 80 00 00`; the low input payload bit is rounded away |
| Changing-limit discard | `C0 01 00 00 00 07 C0 01 00 00 00 00 C0 01 00 00 00 02 80 2E` | Reference: only integer 7 remains. Under the Section 3.2 `[May-improve]` fixed-count rule the stack is empty |
| True loop condition retains its target | `C0 01 00 00 00 12 C0 01 00 00 00 01 80 30` | Only integer target 18 remains; execution continues after the fragment |
| Non-standard push tag `0x8401` | `84 01 00 00 00 05` | A valid push with no error: stack bytes `00 00 00 05 84 01` |
| Non-standard tag accepted by a masked integer check | `C8 01 00 00 00 08 80 04` | `0xC801` with bit `0x0800` cleared is `0xC001`, so the jump is accepted: the stack is empty and the instruction address is 8, the end of the fragment |

### 4.3 Error and Structural Mutations

| Input or mutation | Expected boundary |
| --- | --- |
| Fragment `80 1A` with an empty value stack | Detected stack underflow; VM error |
| Fragment `C0 01 00 00 00 01 C0 01 00 00 00 00 80 3C` | Division-by-zero VM error after consuming the operands |
| Fragment `00 00` | Bad instruction because bit 15 is clear |
| Word `81 56` | Dispatch index 342, outside the reference's array; do not execute as a safe unknown-opcode test |
| Replace the complete fixture's helper body address at `0x0056` with `00 00 02 00` | Out-of-file target; a safe validator rejects it, while the reference lacks a guaranteed clean rejection |
| Replace literal payload length at `0x0076` with `00 00 00 07`, leaving all other bytes unchanged | A strict table validator rejects the inconsistent length/terminator boundary; the reference loader does not use that word to locate execution |
| Set procedure record 1's flags at `0x004A` to `00 00 00 20` | No verified pure-procedure behavior is activated; this bit is not tested by the local call path |
| Fragment `84 01 00 00 00 08 80 04` | The push executes; the jump then raises a VM type error, because `0x8401` with bit `0x0800` cleared is still not `0xC001` |

For the value-stack capacity boundary, execute 341 copies of `C0 01 00 00 00 00` under a fragment harness: they fit in 2046 stack bytes. A 342nd complete push raises stack overflow. This construction is a byte count and repeat rule, not an abbreviated complete script image.

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings

Update notes (2026-10-22): fixture rows for the non-standard push tags `0x8401` and `0xC801` were added to sections 4.2 and 4.3 and stepped by hand through the section 2.5 decode rule. `tools/spec_lint.py` gives 0 errors.
