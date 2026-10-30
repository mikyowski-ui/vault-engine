# SPEC-023: In-Memory VFS Mounting and Portable Storage

**Status:** approved by the Spec-Author sub-agent (Team A), 2026-10-27, under AGENTS.md "Spec Authoring (Team A)". Revisions 2 and 3 (Team A, 2026-10-30); see "Revision history".

**Provenance.** Author: the Spec-Author sub-agent (Team A), 2026-10-27.
Sources:

- the owner's brief for this spec (the Wave 4 prompt, Part B, "Rank 4
  Foundation"), relayed by the coordinator on 2026-10-27:
  1. a byte-slice, in-memory archive backend for `vault_formats::vfs` that
     mounts Fallout 1 `.DAT` (version 1) and Fallout 2 `.DAT` (version 2)
     archives and loose override files directly from in-memory byte buffers,
     without OS filesystem paths, for browser drag-and-drop and embedded
     mobile bundles;
  2. a portable `StorageBackend` trait in `vault_platform` that abstracts
     configuration (`config`), save-game slot reads and writes (`savegame`)
     and mod manifest discovery, so that desktop filesystem I/O and browser
     or mobile key-value storage share one deterministic interface;
  3. hand-checkable synthetic fixtures in `tests/fixtures/spec023/`, every
     rule tagged `[Must-match]`, `[May-improve]` or `[Safe rule]`, and a
     Decisions section;
  4. the coordinator's notes: SPEC-008 and SPEC-015 are the archive format
     sources, cited by section; the fixtures are tiny synthetic archives,
     never game data; engine modules are named by Rust path only, and the new
     interfaces are defined here without reading the engine's code;
- the project's approved specs:
  - SPEC-008: the version 1 archive (sections 2.1 to 2.3), its LZSS (section
    3.1), the selected-database lookup (section 3.2), the staged save rule
    (section 3.3), the save slot files (section 2.4) and the synthetic
    archive (section 4.1);
  - SPEC-015: the version 2 archive, its paths and its zlib members (sections
    1 to 6);
  - SPEC-018: Fallout 2 archive roles and mounting (section 1), finding the
    archives and the required members (sections 2.1 and 2.2), and the
    writable folder and slots (section 2.3);
  - SPEC-020: the single session façade (section 2.3) and the re-recording
    rule (section 3.4);
  - SPEC-021: the state hash (section 2), used by one test;
- the project's documentation in `docs/REFERENCE.md`: the settings file
  `vault-engine.toml` and its search order, `load_order.toml` and mod layers,
  `vault-mod.toml` manifests and where they are read, the loose-directory
  provider's path rules, the staged slot write, the `QUICKSAV` slot and the
  `VAULTENG.TOML` sidecar; and `docs/fallout2-and-qol-roadmap.md` (the `Vfs`
  trait, `DatProvider`, `Dat2Archive`, `Vfs::read_with_layer`);
- public standards: RFC 1950 and RFC 1951 (through SPEC-015), the Adler-32
  checksum of RFC 1950 section 9, and the general shape of browser storage
  (transactional key-value object stores and drag-and-drop file lists, as the
  W3C IndexedDB and File API documents describe them).

No decompilation, disassembly or re-implementation source was consulted, no
game asset was opened, and no engine Rust code was read. Engine modules and
types are named only as the brief and the project documentation name them.
Team A's general knowledge comes from model training data whose sources
cannot be audited (see `PROVENANCE.md`, "In-session spec authoring"). This
spec changes where bytes come from and go to. It adds no game rule.

**Provenance of revision 2.** Author: the Spec-Author sub-agent (Team A),
2026-10-30, on the owner's brief of 2026-10-30, "In-Memory VFS, `StorageBackend`
& `wasm32` Foundation". Sources:

- the owner's brief. It asks for this spec to be written, but revision 1 was
  already approved, and no implementation of it exists yet. So this is
  revision 2 (D-26). Section 6 shows where each item of the brief stands;
- the project's `AGENTS.md` (the engineering gates, `cargo xtask check`) and
  `docs/REFERENCE.md` (the CI jobs, and the `skipped: no GPU adapter` rule of
  SPEC-022 section 4, which the skip rule follows);
- the project's `.github/workflows/ci.yml`;
- public Rust documentation: the `wasm32-unknown-unknown` target's platform
  support page, cargo's `--target`, `--locked`, `--no-default-features` and
  `tree` commands, and `rustc --print target-libdir`;
- **a dependency listing.** Team A printed the workspace's dependency graph with
  `cargo metadata --no-deps` and `cargo tree`: crate names, versions and
  features only (section 3.17). It opened no manifest and no source file under
  the crates or tools;
- **a measurement on the owner's machine, 2026-10-30.** Team A ran
  `rustc --print target-libdir --target wasm32-unknown-unknown` on Rust 1.98.1
  from the distribution, which has no `rustup` and no `wasm32` standard library
  (section 3.17).

No engine Rust code was read for this revision either, and no decompilation,
disassembly, re-implementation source or game asset was used. The new vectors
were written from this text, then checked by a separate script from the written
files alone (section 7).

**Provenance of revision 3.** Author: the Spec-Author sub-agent (Team A),
2026-10-30, on the owner's brief of 2026-10-30, which names this revision.
Sources:

- the owner's brief;
- the implementer's report with the storage work of revisions 1 and 2. It says
  that the save code before this spec removed every `.SAV` of a slot, so the
  premise of D-18 was wrong;
- SPEC-024b section 5.8 (the `VAULTSFL.DAT` sidecar).

No engine Rust code was read, and no decompilation, disassembly,
re-implementation source or game asset was used.

### Revision history

- **Revision 1 (2026-10-27).** First approved text.
- **Revision 2 (2026-10-30).** Adds the offline-safe `wasm32-unknown-unknown`
  compile gate (section 3.17).
  - **The gate.** `vault-formats`, `vault-vm`, `vault-core` and `vault-guidance`
    must pass a compile check for that target, in `xtask` and in CI, with zero
    new dependencies.
  - **The skip.** When the target is not installed, the gate skips with a stated
    line, runs a host check in its place, and never reaches for the network. CI
    cannot skip.
  - **A blocker the gate exposes.** `vault-formats` has a non-optional `cap-std`
    dependency, which cannot build for this target, and `vault-core` and
    `vault-guidance` take `vault-formats` with its default features. So section
    3.6 now also makes `cap-std` optional, and forwards `host-fs` through those
    two crates. Revision 1's `host-fs` alone was not enough.
  - Every other rule of revision 1, and every revision 1 fixture, is unchanged.
- **Revision 3 (2026-10-30).** Restores the stale `.SAV` cleanup.
  - **The rule.** Saving into a slot also deletes every `.SAV` of that slot that
    the new image lacks, in the same commit (section 3.11, D-18). Old map files
    therefore never leak into an overwritten slot, as before this spec.
  - **What stays.** Every file that is not a `.SAV`, including unknown sidecars
    such as `VAULTSFL.DAT` (D-38).
  - **Why.** Revisions 1 and 2 said stale companions stayed "as today". The
    implementer reported that this was wrong, so the code that followed them
    changed the behavior.
  - The new fixture is `slot-saves.tsv`, with test T20. Every other rule and
    fixture is unchanged.

## Clean-Room Rules for Research Agents
- Document ONLY binary layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from any inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

Today the engine reads game data through `vault_formats::vfs` providers that
open OS paths (`LocalDirProvider` for loose folders, `DatProvider` over file
archives), and it reads and writes its settings, load order, mod manifests
and save slots as files. A browser build has no OS paths: game data arrives
as dropped files, and state lives in a key-value store. A mobile build may
ship data inside the application bundle. This spec defines:

1. **In-memory mounting** in `vault_formats::vfs` (section 3.1 to 3.6): a
   shared byte buffer type, version 1 and version 2 archives opened from
   buffers, a loose override tree built from (path, bytes) pairs, a mount
   description that assembles them into the same selected-database namespace
   that file mounts give, and the rules that turn a dropped file list into a
   mount.
2. **Portable storage** in a new module `vault_platform::storage` (sections
   3.7 to 3.15): the `StorageBackend` trait with three namespaces (`config`,
   `savegame`, `mods`), its key grammar, its atomic batch commit, its
   determinism rules, a filesystem implementation that keeps today's desktop
   files byte for byte, a reference in-memory implementation, and the
   key-value layout that browser and mobile hosts use.
3. The tests and synthetic vectors (sections 4 and 5, and
   `tests/fixtures/spec023/`).
4. An offline-safe **`wasm32-unknown-unknown` compile gate** for the portable
   crates (section 3.17; revision 2).

Out of scope: building for a browser or a phone (the gate only compiles; it
links and runs nothing), binding to a particular browser API, caching game
archives across sessions, and every game rule.
Game data is never written to a `StorageBackend` (D-11).

**Compatibility tags.** As in SPEC-020 and SPEC-022, the tags compare the
engine with **its own behavior**, not with the original game:

- `[Must-match]`: a memory mount gives the same file list, the same bytes and
  the same refusals as a file mount of the same bytes, under SPEC-008,
  SPEC-015 and SPEC-018. On the desktop, the files the engine reads and
  writes keep their paths and bytes. The key grammar, the key-value layout,
  the namespaces and the fixture results of this spec are reproduced exactly
  by every backend.
- `[May-improve]`: an implementation freedom (a buffer type, caching, a
  journal format) allowed only if nothing this spec fixes changes.
- `[Safe rule]`: how an input this spec does not accept is handled: it is
  refused with a typed error naming the input, and nothing is partly mounted
  or partly written. Nothing panics, reads out of bounds, or loops without
  bound.

**Notation.** "Canonical path" is the lower-case, `/`-separated form of
SPEC-015 section 4 and the loose provider (section 2.2). Bytes are written in
uppercase hexadecimal. A key or path in prose is shown in canonical form
unless its spelling matters.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Byte Sources

A **byte source** is an immutable, shared byte buffer. `[Must-match]`

| Property | Rule |
| --- | --- |
| Content | A sequence of bytes that never changes after construction |
| Length | 0 to `2^32 - 1` bytes. A longer buffer is refused, because both archive formats address members with 32-bit offsets and sizes (SPEC-008 section 2.2, SPEC-015 section 3) |
| Sharing | Cloning shares the buffer; it never copies it |
| Sub-slices | A sub-range of a byte source is a byte source sharing the same buffer |
| Construction | From an owned vector, from a `'static` slice (an embedded bundle), or from any owner of immutable bytes that can be shared between threads (a memory map, a browser array buffer copied once into the engine) |

The type is `vault_formats::vfs::ByteSource` in Team A's naming (D-21). Its
in-memory representation is free. `[May-improve]`

### 2.2 Canonical Paths and the Path Validator

Loose memory paths, storage keys and drop paths all use one validator, the
one the loose-directory provider applies today (`docs/REFERENCE.md`,
"Loose-directory VFS"). `[Must-match]`

An input path is accepted when all of these hold, and is then canonicalized:

1. It is 1 to 1024 bytes long (SPEC-015 section 4).
2. Every byte is printable ASCII, `0x20` to `0x7E`. A control byte or a
   non-ASCII byte is refused.
3. `/` and `\` are both separators. The path is relative: it does not start
   with a separator, and has no drive prefix (`C:`) or UNC prefix.
4. No component is empty (no repeated or trailing separator), `.` or `..`.
5. No component contains `<`, `>`, `:`, `"`, `|`, `?` or `*`, ends with a
   dot or a space, or is a Windows device name (`CON`, `PRN`, `AUX`, `NUL`,
   `COM1` to `COM9`, `LPT1` to `LPT9`, with or without an extension,
   compared case-insensitively).

The canonical form joins the components with `/` and folds `A`-`Z` to
`a`-`z`. Two paths are the **same path** when their canonical forms are
equal. Canonical paths are ordered by their bytes. `[Must-match]`

If the loose provider's validator accepts or refuses a path differently from
this list, the provider's validator wins, the implementer records the
difference in the commit, and the fixtures are reported for a revision. The
memory path and the storage keys must use the very same validator.
`[Safe rule]`

### 2.3 The Memory Mount

A **memory mount** describes one game's data from byte sources. Its logical
fields: `[Must-match]`

| Field | Type | Meaning |
| --- | --- | --- |
| profile | `fallout1` or `fallout2` | Selects the archive format (SPEC-015 section 1) and the mounting rules |
| master | byte source | The master archive (`MASTER.DAT`) |
| critter | byte source | The critter-art archive (`CRITTER.DAT`) |
| patch | byte source, optional | `patch000.dat`, under `fallout2` only (SPEC-018 section 1). Under `fallout1` a patch is refused |
| loose | memory tree, optional | The loose override tree: the role of `data_root` or `--corpus-dir` (SPEC-008 section 3.2) |
| layers | list of (layer name, memory tree) | Mod layers, lowest first, as a load order lists them |

A **memory tree** is a set of (canonical path, byte source) pairs built from
(path, bytes) input pairs (section 3.3).

A memory mount may also have no archives at all: a **loose corpus**, the
memory form of `--corpus-dir` alone. Its loose tree then serves every
request, as a loose directory mounted alone does today. `[Must-match]`

### 2.4 Storage Namespaces

A `StorageBackend` holds three namespaces. Each key is a canonical path
(section 2.2) with the extra limits of its namespace. `[Must-match]`

| Namespace | Keys | Content | Desktop root (section 3.10) |
| --- | --- | --- | --- |
| `config` | One component ending in `.toml`: `vault-engine.toml`, and the load order file (`load_order.toml` by default) | The settings and load-order text, as bytes | The folder the shell finds or creates `vault-engine.toml` in |
| `savegame` | Exactly two components, `<slot>/<file>`: `SLOT01` to `SLOT10` and `QUICKSAV` are the slots the engine uses | Slot files: `SAVE.DAT`, the `.SAV` companions, `AUTOMAP.SAV`, `VAULTENG.TOML` (SPEC-008 section 2.4), `VAULTSFL.DAT` (SPEC-024b section 5.8) | The active profile's save root, `SAVEGAME/` in its writable data folder (SPEC-018 section 2.3) |
| `mods` | One or more components: `<layer>/<path in the layer>` | Mod layer files, `vault-mod.toml` manifests included | The folder the shell resolves load-order layer paths against |

**Reserved suffixes.** In `config` and `savegame`, a key whose last component
ends in `.tmp` or `.bak` (after case folding) is refused: those names belong
to the staged write (section 3.9). In `mods` they are ordinary names.
`[Safe rule]`

**Limits.** A value is at most 64 MiB (the engine's per-asset limit, SPEC-015
section 3). A longer value refuses its commit. `[Safe rule]`

### 2.5 The Key-Value Layout

A host whose storage is a flat key-value store (a browser object store, a
mobile preferences or database file) keeps each entry as one record:
`[Must-match]`

| Part | Value |
| --- | --- |
| Record key | The ASCII string `vault-engine/1/<scope>/<namespace>/<canonical key>` |
| `<scope>` | `shared` for `config` and `mods`; the profile name, `fallout1` or `fallout2`, for `savegame` |
| `<namespace>` | `config`, `savegame` or `mods` |
| Record value | The entry's bytes, exactly. A store that holds only text keeps the bytes in standard base64 (RFC 4648 section 4, with padding); this encoding is the adapter's and never reaches the engine |

The `1` is the layout version. A later layout uses another number and never
reads or rewrites version 1 records silently. Records under
`vault-engine/1/` with any other shape are ignored by listing and reported
once. `[Safe rule]`

Example (`keys.tsv`): the Fallout 1 slot file `SLOT01/SAVE.DAT` is the record
`vault-engine/1/fallout1/savegame/slot01/save.dat`.

## 3. Behavioral Rules & Edge Cases

### 3.1 Archives From Byte Sources

1. A version 1 archive opens from a byte source by SPEC-008 sections 2.1 to
   2.3 and 3.1, and a version 2 archive by SPEC-015 sections 2 to 5, with
   every `[Must-match]` rule, every `[Safe rule]`, and every `[May-improve]`
   safe rule the file-backed reader applies today. `[Must-match]`
2. **Parity.** For any byte sequence B, opening B from a byte source and
   opening a file holding exactly B give the same result: the same refusal
   (the same error kind and rule), or the same file list in the same order,
   and then, for every path, the same bytes or the same refusal.
   `[Must-match]`
3. Offsets and sizes are checked against the byte source's length, the role
   the file length plays for a file archive. A member whose stored bytes do
   not lie wholly inside the buffer is refused, never read past the end.
   `[Safe rule]`
4. Opening reads only the directory tables or the tree, as the file-backed
   readers do. Members are decoded when they are read. A stored member may
   be returned as a shared sub-slice of the buffer, without a copy.
   `[May-improve]`
5. Decoded members are not cached by the archive. A caller may cache them.
   `[May-improve]`
6. **Format choice.** The profile selects the format: `fallout1` reads only
   version 1 and `fallout2` only version 2 (SPEC-015 section 1). No format
   is guessed from the bytes for mounting. When the master buffer is refused
   under `fallout2` but opens as version 1, the message says that the data is
   Fallout 1 data, as SPEC-018 section 2.1 says for folders; and the reverse
   for Fallout 2 data under `fallout1`. This probe only chooses the message.
   `[Safe rule]`

Team A names the constructors `DatArchive::from_bytes(ByteSource)` and
`Dat2Archive::from_bytes(ByteSource)` (D-21). The existing file and stream
constructors stay.

### 3.2 Assembling the Namespace

A memory mount is assembled into the same provider structure that a file
mount builds today, and answers requests by the same rules: `[Must-match]`

1. **Databases.** The master database is the loose tree, then (under
   `fallout2`) the patch archive, then the master archive. The critter
   database is the loose tree, then the critter archive. A request under
   `art/critters/` uses the critter database; every other request uses the
   master database. There is no fallback from one database to the other
   (SPEC-008 section 3.2, SPEC-018 section 1).
2. **Mod layers.** Mod layers lie above everything else, highest layer first,
   as file-backed mod layers do today. A layer serves a request when it holds
   the path.
3. **Requests.** A request path is validated and canonicalized (section
   2.2), so both slash styles and any letter case are accepted, as the loose
   provider accepts them. Archive lookup then applies SPEC-008 section 3.2
   (version 1) or SPEC-015 section 4 (version 2) to the canonical path, as
   the file-backed provider does.
4. **Listing.** The provider lists the union of the paths every part serves,
   in canonical order, as the file mount does.
5. **Layer names.** `Vfs::read_with_layer` names the part that served a read:
   `loose`, `patch`, `master`, `critter`, or the mod layer's name. If the
   existing provider uses other labels for the same parts, the test maps
   them. `[May-improve]` for the label text.
6. **Validation.** SPEC-018 section 2.2's required members, and Fallout 1's,
   are checked on the assembled namespace exactly as for a file mount.
   Section 2.1 of SPEC-018 (finding archives in a folder) is replaced by the
   drop rules of section 3.5 for memory mounts.

Team A names the assembly `MemoryMount::mount` (D-21).

### 3.3 Loose Trees in Memory

A memory tree is built from a list of (path, bytes) pairs: `[Safe rule]`

1. Each path is validated and canonicalized (section 2.2). An invalid path
   refuses the tree, naming the path.
2. Two pairs with the same canonical path refuse the tree, naming both input
   spellings. The loose provider refuses case collisions the same way.
3. A canonical path that is also a folder of another path (`data` and
   `data/a.txt`) refuses the tree, naming both. A filesystem cannot hold
   both.
4. An empty value is an ordinary empty file. A value above 64 MiB refuses
   the tree.

Reads of a memory tree return the stored bytes. Its listing is its canonical
paths in canonical order. Unlike a loose folder, a memory tree never changes
after it is built, so there is no live re-resolution. `[Must-match]`

Team A names the type `vault_formats::vfs::MemoryDir` (D-21).

### 3.4 Mod Layers in Memory

A mod layer from storage (section 3.12) is a memory tree built from the
layer's entries, with paths relative to the layer. Its `vault-mod.toml`
manifest, `[[scripts]]`, `[[maps]]` and `[[patches]]` registrations, and the
mod-script verification on load work exactly as for a folder layer, because
they read through the `Vfs` interface. `[Must-match]`

### 3.5 Building a Mount From Dropped Files

A host that receives files by drag-and-drop, or a file picker, gets a list of
(relative path, bytes) pairs. They become a memory mount by these rules, in
order. `[Safe rule]` for the refusals, `[Must-match]` for the result.

1. **Validate.** Every dropped path is validated and canonicalized (section
   2.2). An invalid path refuses the whole drop, naming it.
2. **Strip one folder.** If every path has at least two components and all
   share the same first component, that component is removed from every
   path once. (Dropping the game folder itself.)
3. **Archives.** Paths of exactly one component named `master.dat`,
   `critter.dat` or `patch000.dat` (compared after case folding) are the
   archives. Two dropped entries that fold to the same archive name refuse
   the drop, naming both, as SPEC-018 section 2.1 refuses two entries that
   differ only in case. Under `fallout1`, `patch000.dat` is not mounted and
   is reported as ignored.
4. **Loose tree.** If at least one archive was found, the loose tree is the
   entries under the top folder `data/`, with that folder removed from their
   paths (the conventional `data_root`), except those under `data/savegame/`.
   Every other entry is reported as ignored. If no archive was found, every
   entry is the loose tree (a loose corpus).
5. **Required archives.** When any archive was found, both `master.dat` and
   `critter.dat` must be present; a missing one refuses the drop, naming it
   (SPEC-018 section 2.1).
6. **Open.** The mount is assembled (section 3.2) and validated. Every
   refusal names the dropped entry and the rule.

The report of a drop lists, in canonical order, the archives found, the
number of loose files, and every ignored entry. It is deterministic: the same
list of pairs, in any order, gives the same mount and the same report.
`[Must-match]`

Team A names the function `vault_formats::vfs::classify_drop` (D-21).

### 3.6 No Filesystem on the Memory Path

1. The memory types of sections 2.1 to 3.5 never open, stat, list or create
   an OS path. `[Must-match]`
2. `vault-formats` gains a default cargo feature, `host-fs`, that gates the
   file-backed providers and constructors. With `--no-default-features` the
   crate builds and the memory tests (section 4, T1 to T7) pass. Desktop
   builds are unchanged, because the feature is on by default.
   `[Must-match]` for the build, `[May-improve]` for the feature's name.
3. **`cap-std` (revision 2).** `host-fs` also gates the crate's `cap-std`
   dependency, which becomes optional. It is `vault-formats`' only dependency
   that reaches the OS, and it does not build for
   `wasm32-unknown-unknown`, so the gate of section 3.17 cannot pass while it
   is unconditional. `[Must-match]`
4. **Forwarding (revision 2).** `vault-core` and `vault-guidance` take
   `vault-formats` with its default features off, and each gains its own default
   `host-fs` feature that turns on `vault-formats/host-fs`. Without this, those
   crates would turn `cap-std` back on through their dependency. Every other
   crate of the workspace keeps the file-backed code through its own default
   features, so desktop builds, `Cargo.lock` and every test are unchanged.
   `[Must-match]`
5. **Gated code (revision 2).** Code in `vault-core` and `vault-guidance` that
   uses a file-backed provider goes behind their `host-fs`, as the save-slot
   code's move to slot images already requires (section 3.11). `[Must-match]`

### 3.7 The StorageBackend Trait

`vault_platform::storage::StorageBackend` has these operations. Each takes a
namespace (`Config`, `Savegame` or `Mods`). `[Must-match]`

| Operation | Result |
| --- | --- |
| `read(ns, key)` | The entry's bytes, or "absent". An invalid key is an error |
| `list(ns, prefix)` | The canonical keys of every entry whose key is the prefix's components followed by at least one more component, in canonical order. An absent prefix lists the whole namespace. A prefix matches whole components only: `slot0` matches nothing under `slot01/` |
| `commit(ns, ops)` | Applies a batch of `put(key, bytes)` and `delete(key)` operations atomically (section 3.8) |
| `flush()` | Returns when every committed batch is durable, or reports the first one that could not be made durable (section 3.13) |
| `mount_layer(layer)` | The `mods` layer whose key prefix is `layer`, as a `Vfs` provider (section 3.12) |

**Keys.** A key keeps the spelling its caller gave, and compares by its
canonical form. `read` accepts any spelling of a key. `list` returns
canonical keys. `[Must-match]`

**Errors.** One closed error type: `InvalidKey` (with the rule), `TooLarge`,
`QuotaExceeded`, `Collision` (two stored entries share a canonical key),
`LayerNotFound`, `Unavailable` (the store failed or was closed) and `Io`
(with the host's message). Host error details appear only in messages.
`[Must-match]` for the kinds.

### 3.8 Commit Semantics

1. **Validation first.** Every operation's key is validated for its namespace
   (sections 2.2 and 2.4), every value checked against the size limit, and
   the quota (if any) checked against the batch's net effect. Any failure
   refuses the whole batch, and nothing changes. `[Must-match]`
2. **Net effect.** Operations apply in order: a later operation on the same
   canonical key replaces an earlier one. Deleting an absent key does
   nothing. `[Must-match]`
3. **Atomic.** After a commit returns success, every operation is visible to
   `read` and `list`. After a failure, none is. `[Must-match]`
4. **Spelling.** A `put` on a key that already exists keeps the stored
   spelling where the backend keeps spellings (the filesystem). A new key is
   stored with the caller's spelling there. Key-value backends store only the
   canonical form. `[Must-match]`
5. **Quota accounting.** A backend with a quota counts, over all its
   namespaces, the sum of (canonical key length + value length) of every
   entry. A batch whose net effect would raise this above the quota is
   refused with `QuotaExceeded`; equal to the quota is allowed. Key-value
   hosts map their own quota errors to `QuotaExceeded`. `[Must-match]` for
   the reference in-memory backend; `[May-improve]` for a host's own
   accounting.

### 3.9 The Filesystem Backend

`FsStorage` keeps the desktop's files where and as they are today.

1. **Roots.** Each namespace maps to one folder (section 2.4, table). A key's
   components are the folder and file names below that root. `[Must-match]`
2. **Reads.** A key is resolved case-insensitively, component by component,
   as the loose provider resolves reads. Two entries of one folder that fold
   to the same name give `Collision`. Symbolic links and special files inside
   the root are refused with the loose provider's rules, and nothing outside
   the root is ever read or written. `[Safe rule]`
3. **Listing.** Only regular files are listed; folders are not entries. In
   `config` and `savegame`, names with a reserved suffix (`.tmp`, `.bak`) are
   staging leftovers: they are not listed and cannot be read. In `config`,
   only top-level `*.toml` files are listed. `[Must-match]`
4. **Commit.** The batch is applied with the staged protocol the save path
   uses today (SPEC-008 section 3.3's safe rule, `docs/REFERENCE.md`): each
   put is written to a staging name; when all are written, each target that
   exists (puts and deletes) is renamed to its backup name; the staged files
   are renamed into place; then the backups are removed. If a step fails,
   the staged files are removed and the backups restored. Missing parent
   folders are created. A folder left empty by deletes is kept.
   `[Must-match]` for the protocol and the resulting files; the staging and
   backup names are those the save path uses today.
5. **Crash window.** A crash during the rename step can leave backups on disk,
   as today. They are invisible to `read` and `list`, and the next commit
   that touches the key replaces them. Recovering them automatically is an
   allowed improvement. `[May-improve]`
6. **Durability.** Whether a commit syncs file contents and folders to disk
   is the implementation's choice; `flush` returns at once. `[May-improve]`

### 3.10 The `config` Namespace

1. `vault_platform::config` reads and writes `vault-engine.toml`, and the
   load order file, through the `config` namespace. The text grammar, the
   kept unknown keys and every value rule are unchanged. `[Must-match]`
2. **Desktop root.** The desktop shell resolves the config root as it finds
   the settings file today: beside the executable, then in the working
   directory; and when it creates one, in the same order. The same file is
   read, and Save rewrites it in place. `[Must-match]`
3. **Load order.** A `mod_list` naming a file is a key of the `config`
   namespace on hosts without OS paths. On the desktop it keeps its current
   meaning. `[Must-match]`
4. **Path keys on memory hosts.** On a host whose game data is a memory
   mount, the keys `master_dat`, `critter_dat`, `data_root` and the
   `[profiles.*]` paths are not used. They are kept unchanged when the file
   is rewritten, as unknown keys are. `[Safe rule]`
5. A commit of the settings file is one batch with one put, so a failed
   write leaves the old file whole. `[Must-match]`

### 3.11 The `savegame` Namespace and Slot Images

1. **Slot images.** The save-slot code produces, for a save, a **slot
   image**: the set of (file name, bytes) of every file it writes today
   (`SAVE.DAT`, the companions, `AUTOMAP.SAV`, and `VAULTENG.TOML` when an
   enhanced save keeps one). For a load it consumes a slot image read from
   storage. It no longer opens files itself. The bytes of every file are
   unchanged. `[Must-match]`
2. **Save.** Saving into slot S lists the slot's entries, then makes one commit
   of: `[Must-match]`
   - a put of `S/<name>` for every file of the image;
   - a delete of `S/VAULTENG.TOML` when the image has none, as an overwritten
     slot loses a stale sidecar today;
   - a delete of every **stale `.SAV`** (revision 3): each listed entry of S
     whose file name ends in `.SAV` after case folding, and whose canonical key
     no file of the image has.

   Every other existing file of the slot is left alone, whatever its name. An
   unknown sidecar such as `VAULTSFL.DAT` is one of these: this rule never
   deletes it (D-38). The rest of the rule is as follows:
   - **Names.** The test is on the name alone, not on the loader's stem check, so
     a stale `OLD MAP.SAV` goes too. `DEN.SAV.OLD` and `DEN.SAVE` do not end in
     `.SAV`, and stay. `AUTOMAP.SAV` is always in the image, so it is never stale.
   - **Scope.** Only slot S is touched, on every backend, and `QUICKSAV` is a slot
     like any other. A folder is not an entry (section 3.9), so it stays.
   - **Failure.** If the listing or the commit fails, the save fails and the slot
     is unchanged, stale files included (section 3.8).
3. **Load.** The loader reads `S/SAVE.DAT`, then the companions the manifest
   names, under today's name check (a plain `[A-Za-z0-9_-]` stem plus
   `.SAV`), `S/AUTOMAP.SAV`, and `S/VAULTENG.TOML` if present. It validates
   all of them before any state changes, as today. A missing file is the
   same error as a missing file today. `[Must-match]`
4. **Slots present.** `slots()` lists `SLOT01` to `SLOT10`, then `QUICKSAV`,
   keeping only those whose `SAVE.DAT` exists. Other folders of the save
   root, and slot folders without `SAVE.DAT`, are not slots. The save and
   load screens read headers from there as today. `[Must-match]`
5. **Profiles.** The namespace's root belongs to the active profile: the
   profile's save root on the desktop, the profile's scope in a key-value
   store (section 2.5). The SPEC-018 section 2.3 header check still refuses
   the other game's slots. `[Must-match]`
6. **Layering.** `vault_core` does not depend on `vault_platform`, so the
   slot code exchanges slot images, and `vault_platform::session` moves
   them to and from the `StorageBackend`. Tools that save outside a session
   (the `asset-audit` save validation, for example) may keep a helper that
   commits a slot image to a folder through `FsStorage`. `[May-improve]`
   for the helper.

Team A names the type `SlotImage` (D-21); it lives beside the save-slot code.

### 3.12 The `mods` Namespace and Manifest Discovery

1. **Layers.** A layer is a key prefix of the `mods` namespace: the layer's
   path as the load order writes it, canonicalized. Its files are the
   entries under that prefix, with the prefix removed. `[Must-match]`
2. **Manifests of a load order.** For each layer of a parsed load order, in
   load-order order, enabled or not, discovery reads `<layer>/vault-mod.toml`
   and returns its bytes, or "no manifest". The manifest is parsed by
   `vault_platform::mods` as today. `[Must-match]`
3. **Finding layers.** `find_layers()` returns, in canonical order, every
   prefix P such that `P/vault-mod.toml` exists. A `vault-mod.toml` at the
   namespace root is not a layer (a layer's name is never empty). A launcher
   without a file browser offers these as layers to add. `[Must-match]`
4. **Mounting.** `mount_layer(P)` gives a provider over the layer's files.
   On the desktop it is the loose folder provider over the folder P, as
   today. On other backends it is a memory tree (section 3.4). A layer with
   no file is `LayerNotFound` on every backend, an empty folder included.
   `[Must-match]`; refusing an empty desktop folder is `[May-improve]`
   (D-19).
5. **Writes.** `cargo xtask export-mod` and the in-game export write a mod
   folder through one `mods` commit on hosts without OS paths. On the
   desktop they keep writing the folder they write today. `[Must-match]`

### 3.13 Determinism and Asynchronous Hosts

1. **One interface.** For the same sequence of operations from the same
   empty state, every backend gives the same results: the same bytes, the
   same absences, the same listings in the same order, and the same error
   kinds. No operation exposes a timestamp, a file size on disk, a host
   listing order or an inode. `[Must-match]`
2. **No influence on play.** Which backend holds the slots never changes a
   game state: a slot saved on one backend and copied entry for entry to
   another loads to the same SPEC-021 `state_hash`. `[Must-match]`
3. **Synchronous trait.** The trait is synchronous. A host whose store is
   asynchronous (a browser object store) loads its three namespaces into
   memory when it opens, before the session starts; serves reads and lists
   from memory; applies each commit to memory atomically and queues one host
   transaction per commit, in commit order; and makes `flush` wait for the
   queue. `[Must-match]` for the visible behavior; `[May-improve]` for the
   mechanism.
4. **Failed flush.** If a queued transaction fails, `flush` reports it, the
   session shows the failure once, and every later commit returns
   `Unavailable` until the store is reopened, so the engine never reports a
   later save as kept. `[Safe rule]`
5. **Atomic host writes.** Each queued transaction is atomic in the host
   store. A store without transactions uses a journal (write the batch, apply
   it, remove the journal; replay a journal found when opening). The journal
   is the adapter's own; its record key starts with `vault-engine/1/journal`,
   which listing never returns. `[May-improve]` for its format.

### 3.14 The Reference Backends

1. `MemoryStorage` holds the three namespaces in memory, with an optional
   quota. It is the reference for the conformance fixtures, and the backend
   of tests and replays, so that they never touch the disk. `[Must-match]`
   for its behavior; `[May-improve]` for using it in replays.
2. `KvStorage` adapts any flat key-value store with get, put, delete, a
   prefix scan and an atomic multi-record write to the trait, using the
   layout of section 2.5. Browser and mobile hosts implement only the store.
   `[May-improve]` for the split.
3. `FsStorage` is section 3.9.

### 3.15 Session Wiring

1. `vault_platform::session`'s `SessionRuntime` receives its game data as a
   `Vfs` and its `StorageBackend` from the entry point, and uses only those
   two for game data, settings, load order, manifests and slots. Every entry
   point of SPEC-020 section 2.3 (launcher, command line, replay) builds them
   and then runs the same steps. `[Must-match]`
2. The desktop entry points build file mounts and `FsStorage`; their files,
   paths and bytes stay as they are, so the gallery and the replay logs stay
   bit-identical (SPEC-020 section 3). `[Must-match]`
3. A replay never reads or writes the user's real storage. `[Safe rule]`

### 3.16 Module Placement and Names

`[May-improve]` unless tagged.

- `ByteSource`, `MemoryDir`, `MemoryMount`, `classify_drop` and the
  `from_bytes` constructors belong in `vault_formats::vfs` (and its archive
  modules).
- `StorageBackend`, the namespaces, keys, errors, `FsStorage`,
  `MemoryStorage` and `KvStorage` belong in the new module
  `vault_platform::storage`. `vault_platform::config`, `vault_platform::mods`
  and `vault_platform::session` use it.
- `SlotImage` lives beside the save-slot code.
- The names are Team A's (D-21). An implementer may rename them, and records
  the mapping in the commit.
- The gate of section 3.17 is `cargo xtask wasm-check`, in the `xtask` tool, as
  a pure decision function and a thin runner (Team A's names, D-36).

### 3.17 The `wasm32-unknown-unknown` Compile Gate (Revision 2)

The browser build is out of scope, but the crates it needs must not drift away
from it. This gate keeps them compiling for that target. It is a **compile check
only**: it links nothing and runs nothing.

**The crates.** Four crates are gated. `[Must-match]`

| Order | Crate | Why it is portable |
| ---: | --- | --- |
| 1 | `vault-vm` | The VM has no dependency on the host |
| 2 | `vault-formats` | Parsers, archives and the memory mount, with `host-fs` off (section 3.6) |
| 3 | `vault-guidance` | Campaign guidance, from `vault-formats` |
| 4 | `vault-core` | The deterministic world |

`vault-render`, `vault-debug`, `vault-platform`, `xtask`, `asset-audit` and
`vault-fixtures` are not gated. They need a GPU, a window, audio or a filesystem,
and a browser host is a later spec's work.

**The command.** For each crate in that order, one invocation, so that the
features of one crate cannot unify with another's: `[Must-match]`

`cargo check --target wasm32-unknown-unknown --locked --lib --no-default-features -p CRATE`

The gate stops at the first crate that fails. Only library targets are checked:
tests and fixtures may use the filesystem.

**Zero new dependencies.** `[Must-match]`

1. This spec adds no crate to `Cargo.lock`, and changes no version in it. Making
   `cap-std` optional changes no locked package. The gate runs `--locked`, so a
   changed lock file fails it.
2. With `host-fs` off, each gated crate's normal dependencies for this target are
   exactly those of `wasm-graph.tsv`. The list is an allow-list, read from the
   workspace's graph on 2026-10-30.
3. Nothing is added for this target. In particular there is no `wasm-bindgen`,
   `js-sys`, `web-sys`, `getrandom` or panic hook, and no dependency section
   conditioned on the target.
4. Every crate on the allow-list is already in the host graph. So a machine that
   has built the workspace has every crate the gate needs, and the gate needs no
   download.

**One code path.** A gated crate does not branch on `target_arch` or `target_os`
for this target. It branches on the `host-fs` feature. So the code the gate
compiles is the code the host tests run with `--no-default-features` (T7), and
the gate cannot pass on a stub. `[Must-match]`

**Finding the target.** `[Must-match]`

1. The gate runs `$RUSTC` (or `rustc` when that is unset) with
   `--print target-libdir --target wasm32-unknown-unknown`.
2. The target is **installed** exactly when that command exits 0, the path it
   prints is a directory, and that directory holds a file whose name starts with
   `libcore-` and ends with `.rlib`.

The directory test is needed: on the owner's machine, Rust 1.98.1 from the
distribution prints `/usr/lib/rustlib/wasm32-unknown-unknown/lib` and exits 0,
and that directory does not exist. The exit status alone would call the target
installed (measured by Team A, 2026-10-30).

**Offline safety.** The gate never installs anything. It never runs `rustup`,
which a machine may not have, and it makes no network request of its own. A
machine without the target and without a network stays useful. `[Must-match]`

**Outcomes.** `VAULT_REQUIRE_WASM` follows SPEC-022's `VAULT_REQUIRE_GPU`: only
the value `1` requires the target. `wasm-gate.tsv` lists these outcomes as
vectors. The lines are exact. `[Must-match]`

| Target | `VAULT_REQUIRE_WASM` | What runs | Output | Exit |
| --- | --- | --- | --- | ---: |
| Not installed | Anything but `1` | The host check below | `skipped: wasm32-unknown-unknown target not installed`, a free-form line saying how to install it, then `host check ok: 4 crates` | 0 |
| Not installed | `1` | Nothing | `error: wasm32-unknown-unknown target not installed` | 1 |
| Installed | Any | The four checks | `wasm-check ok: 4 crates` | 0 |
| Installed | Any | The four checks, until one fails | Cargo's own errors, then `wasm-check failed: CRATE` | 1 |

**The host check.** A skip still checks what it can. It runs the command above
without `--target`, for the same four crates in the same order. That proves on
every machine that the `host-fs` gating holds, though not that the target
compiles. If it fails, the gate prints `wasm-check failed: CRATE` and exits 1,
because a broken feature gate is a real failure, whatever the target. `[Must-match]`

**A failure is not a skip.** With the target installed, any failure of a check
fails the gate, a failed download included. Only a missing target skips, so a
skip always means the same thing. `[Must-match]`

**Where it runs.** `[Must-match]`

- `cargo xtask wasm-check` runs the gate alone.
- `cargo xtask check` runs it as a fourth step, after the three gates of
  `AGENTS.md`. A skip does not fail `cargo xtask check`, and its line stays
  visible in the output.
- CI gains a job, `wasm-check`, on Linux. It installs the stable toolchain with
  the target, through the workflow's existing toolchain action, which is pinned
  by version tag like the others. It sets `VAULT_REQUIRE_WASM=1` and runs
  `cargo xtask wasm-check`. So CI can never pass by skipping.

**What the gate does not prove.** On this target `std::fs` compiles but every
call fails at run time, `Instant::now` and `SystemTime::now` panic, and a thread
cannot start. The gate cannot see that. So with `host-fs` off, a gated crate does
not call `std::fs`, `std::net`, `std::process`, `std::thread::spawn`,
`Instant::now` or `SystemTime::now` on any path. A use that exists today goes
behind `host-fs`, or moves to the platform. Enforcing this with clippy's
`disallowed_methods` is recommended. `[Must-match]` for the rule; `[May-improve]`
for the enforcement.

**32-bit sizes.** `usize` is 32 bits on this target. `[Safe rule]`

- Archive and memory-mount arithmetic that adds a 32-bit offset and size is
  checked, or done in u64, so that an entry near `2^32` is refused the same way
  on every target. It never wraps.
- A byte source cannot reach section 2.1's limit of `2^32 - 1` bytes there,
  because a buffer is limited to `isize::MAX`. Such a buffer never exists, so no
  extra refusal is needed. A failed allocation is the host's.
- SPEC-021's streams hold no `usize`, so `state_hash` does not depend on the
  target's width (SPEC-021 section 2.2).

### 3.18 The Implementer's Order (Revision 2)

The gate fails until section 3.6 is done, so the order matters. `[Must-match]`

1. **Section 3.6 first:** `host-fs` in `vault-formats`, `cap-std` optional,
   `vault-core` and `vault-guidance` forwarding the feature, and their file-backed
   code behind it.
2. **Then the memory path** (sections 3.1 to 3.5), with T1 to T7.
3. **Then the gate** (section 3.17), with T16 to T19, and the CI job in the same
   commit. It must not land before step 1: `cargo xtask check` would then fail on
   every machine that has the target.
4. **Then storage** (sections 3.7 to 3.15), with T8 to T15.
5. **Documents.** Add the fourth step and the CI job to `AGENTS.md` and
   `docs/REFERENCE.md`, which list the gates today.

### 3.19 The Revision 3 Follow-Up

The engine implements revision 2's rule today, so revision 3 needs this work:
`[Must-match]`

1. `vault_platform::session` lists the slot and adds the deletes of section 3.11
   rule 2 to the save's commit. So does any helper that commits a slot image.
2. T20 is added.
3. `docs/REFERENCE.md`, which says other files of a slot are left alone, gains
   the rule.

## 4. Required Tests

The implementer writes at least these tests, on synthetic data only.
`[Must-match]`

- **T1. Version 1 from bytes.** `f1-master.dat.hex` and `f1-critter.dat.hex`
  open from byte sources and list and read the members of section 5.1. The
  same bytes written to a temporary file and opened by the file-backed reader
  give the same lists and bytes.
- **T2. Version 2 from bytes.** The same for `f2-master.dat.hex`,
  `f2-patch.dat.hex` and `f2-critter.dat.hex` (section 5.2), including the
  zlib member `data/zip.txt` (`HELLO`).
- **T3. Lookups.** The mounts `f1`, `f2` and `f2-nopatch` of section 5.3
  give every row of `lookups.tsv`: the bytes and the serving part, or "not
  found".
- **T4. Refusals.** Every row of `refusals.tsv` is refused at the stated
  stage, with no panic. For rows 1 to 7, the file-backed reader refuses the
  same bytes with the same error kind.
- **T5. Parity on existing fixtures.** Every archive fixture of
  `tests/fixtures/archive/` (SPEC-008 section 4.1) and the version 2
  archives built from `tests/fixtures/zlib/` give the same results from
  bytes and from files.
- **T6. Drops.** Every row of `drops.tsv` gives the stated mount, report or
  refusal, for the entries in the stated order and in reverse order.
- **T7. No filesystem.** `cargo test -p vault-formats --no-default-features`
  builds and runs T1 to T6. (If the crate's other tests need `host-fs`, they
  are gated on it.)
- **T8. Keys.** Every row of `keys.tsv`: the canonical key and the
  key-value record key, or the refusal.
- **T9. Conformance.** `storage-ops.tsv` runs on `MemoryStorage`, on
  `FsStorage` over an empty temporary folder, and on `KvStorage` over a
  test store, with the same results at every step. Afterwards the
  `FsStorage` folder holds no staging or backup file.
- **T10. Quota.** `quota.tsv` on `MemoryStorage` with a quota of 20 bytes.
- **T11. Mods.** With the entries of `mods-entries.tsv` in the `mods`
  namespace, every row of `mods-expected.tsv` holds, on `MemoryStorage` and
  on `FsStorage`.
- **T12. Atomic filesystem commit.** A fault injected while staging the
  second file of a slot commit leaves the slot's files unchanged and no
  staged file behind.
- **T13. Desktop parity.** A save, a quick save and a settings save through
  `SessionRuntime` with `FsStorage` write the same paths and bytes as before
  this spec (compare with a save made by the code before the change, or with
  a recorded byte digest). `cargo xtask capture-gallery && git diff
  --exit-code docs/assets` passes, and the replay logs are unchanged.
- **T14. Cross-backend load.** A slot saved on `MemoryStorage` from the
  synthetic map (`cargo xtask fixtures`), copied entry for entry to
  `FsStorage`, loads to the same SPEC-021 `state_hash` as loading it from
  `MemoryStorage`.
- **T15. Asynchronous adapter.** A test store that completes transactions
  later shows: reads see a commit at once; transactions reach the store in
  commit order; a failed transaction makes `flush` report it and later
  commits return `Unavailable`.
- **T16. Gate decisions (revision 2).** The gate's decision function gives
  every row of `wasm-gate.tsv`. The probe is tested on temporary directories: one
  missing, one empty, and one holding a file `libcore-0.rlib` (an empty file is
  enough, because only the name is tested). This test needs no target, and runs
  everywhere.
- **T17. The real skip (revision 2).** Run `cargo xtask wasm-check` with `RUSTC`
  naming a small test wrapper that prints a missing path for `--print
  target-libdir`, and hands every other call to the real compiler. With
  `CARGO_NET_OFFLINE=true` the gate prints the skip line, runs the host check, and
  exits 0. With `VAULT_REQUIRE_WASM=1` it exits 1 and runs no check.
- **T18. The compile check (revision 2).** With the target installed, the four
  crates pass, in order. The CI job runs this with `VAULT_REQUIRE_WASM=1`.
- **T19. The dependency graph (revision 2).** For each row of `wasm-graph.tsv`,
  `cargo tree -e normal --target wasm32-unknown-unknown --no-default-features -p
  CRATE --prefix none --locked` lists exactly the row's workspace and allowed
  crates, by name. This needs no installed target (measured by Team A), so it runs
  everywhere and catches a new dependency on a machine that skips the gate.
  `Cargo.lock` is unchanged by this spec's work.
- **T20. Slot saves (revision 3).** On `MemoryStorage`, `FsStorage` and
  `KvStorage`, each row of `slot-saves.tsv` (section 5.9) starts from its `before`
  entries, commits its `image` to its `slot` by section 3.11 rule 2, and gives its
  `after` entries exactly. On `FsStorage`, a fault injected as in T12 during the
  commit of `stale-map` leaves all four files of the slot unchanged, and a folder
  named `OLD.SAV` in a slot survives a save.

## 5. Synthetic Test Fixtures (Hex / Text)

All values are synthetic and every file is in `tests/fixtures/spec023/`,
which has a README. Team A computed the archives with a throwaway script that
is not committed, and checked every offset below by hand. Hex files use the
`OFFSET: bytes` form of SPEC-008 section 4.1, 16 bytes per line.

### 5.1 Fallout 1 Archives (Version 1, Big-Endian)

`f1-master.dat.hex` (217 bytes) has three directories, `.`, `ART\CRITTERS`
and `DATA`, sorted case-insensitively:

| Directory | File | Attributes | Offset | Logical | Packed | Content |
| --- | --- | --- | ---: | ---: | ---: | --- |
| `.` | `README.TXT` | `0x20` | `0xBD` | 7 | 7 | `MASTER1` |
| `ART\CRITTERS` | `ONLYM.FRM` | `0x20` | `0xC4` | 4 | 4 | `MFRM` |
| `DATA` | `PACK.TXT` | `0x40` | `0xC8` | 9 | 8 | One final block `00 06 07 41 42 43 EE F3`, which SPEC-008 section 4.2 decodes to `ABCABCABC` |
| `DATA` | `SHARED.TXT` | `0x20` | `0xD0` | 9 | 9 | `M1-SHARED` |

Walk: the root prefix (count 3, capacity 3, associated size 0, opaque 0) is at
`0x00`; the names `01 2E`, `0C ART\CRITTERS`, `04 DATA` follow at `0x10`. The
first file table starts at `0x24`, the second at `0x4F`, the third at `0x79`.
`README.TXT`'s entry starts at `0x34`: length `0A`, the name, attributes
`00 00 00 20` at `0x3F`, offset `00 00 00 BD` at `0x43`. `PACK.TXT`'s entry
starts at `0x89`; its logical length `00 00 00 09` is at `0x9A`.

`f1-critter.dat.hex` (132 bytes): `ART\CRITTERS\TEST.FRM` = `C1FRM` and
`DATA\SHARED.TXT` = `C1-SHARED`, both `0x20`.

`f1-loose.tsv`: `Data/Shared.TXT` = `LOOSE`, `art\critters\loose.frm` =
`LFRM`, `MAPS/note.txt` = `NOTE` (canonical `data/shared.txt`,
`art/critters/loose.frm`, `maps/note.txt`).

### 5.2 Fallout 2 Archives (Version 2, Little-Endian)

`f2-master.dat.hex` (142 bytes): member data from `0x00`, the tree from
`0x1E`, and the footer at `0x86`: `tree_size` `68 00 00 00` (104) and
`file_size` `8E 00 00 00` (142). `0x8E - 8 - 0x68 = 0x1E`, the tree's start.

| Path (stored) | Flags | Real | Packed | Offset | Content |
| --- | ---: | ---: | ---: | ---: | --- |
| `art\critters\onlym.frm` | 0 | 5 | 5 | `0x00` | `M2FRM` |
| `data\shared.txt` | 0 | 9 | 9 | `0x05` | `M2-SHARED` |
| `data\zip.txt` | 1 | 5 | 16 | `0x0E` | A zlib stream of `HELLO` |

The zlib member is hand-checkable: `78 01` (CMF 8 with window 7; `0x7801` =
30721 = 31 × 991), then one final stored DEFLATE block `01`, `LEN` `05 00`,
`NLEN` `FA FF`, the bytes `48 45 4C 4C 4F`, and the big-endian Adler-32
`04 4C 01 75`: a = 1 + 72 + 69 + 76 + 76 + 79 = 373 (`0x175`), b = 73 + 142 +
218 + 294 + 373 = 1100 (`0x44C`). The checksum's last byte is at `0x1D`.

`f2-patch.dat.hex` (94 bytes): `data\shared.txt` = `P2-SHARED` and
`data\patchonly.txt` = `P2ONLY`. `f2-critter.dat.hex` (55 bytes):
`art\critters\test.frm` = `C2FRM`.

`f2-loose.tsv`: `data/PatchOnly.txt` = `LOOSE2`, `text/note.txt` = `NOTE2`.

### 5.3 Lookups

The mounts are `f1` (profile `fallout1`: `f1-master`, `f1-critter`,
`f1-loose`), `f2` (`fallout2`: `f2-master`, `f2-patch`, `f2-critter`,
`f2-loose`) and `f2-nopatch` (`f2` without the patch). From `lookups.tsv`:

- `data/shared.txt` under `f1` is `LOOSE`: the loose tree comes first in the
  master database. Requested as `DATA\SHARED.TXT`, it is the same file.
- `art/critters/onlym.frm` is "not found" under `f1` and `f2`: only the
  master archive has it, and critter requests never fall back to the master
  database.
- `art/critters/loose.frm` under `f1` is `LFRM`: the critter database
  consults the loose tree first.
- `data/shared.txt` under `f2` is `P2-SHARED` (the patch over the master
  archive), and `M2-SHARED` under `f2-nopatch`. `data/patchonly.txt` is
  `LOOSE2` in both: the loose tree is above the patch.

### 5.4 Refusals

`refusals.tsv` lists 13 cases. Rows 3 to 7 name single-byte mutations of the
fixture archives, by offset: row 3 makes `file_size` 143 for a 142-byte
buffer; row 5 breaks the Adler-32 (the archive opens, the member read fails);
row 6 is SPEC-008 section 4.1's "logical length 10 without a tenth byte";
row 7 moves `README.TXT`'s payload to `0xFD`, past the 217-byte buffer. Rows
8 to 13 are memory-tree refusals (section 3.3).

### 5.5 Drops

`drops.tsv` lists 9 drops. Case 1 strips the common folder `fallout/`, finds
both archives, mounts `data/maps/x.map` as the loose `maps/x.map`, and
ignores `data/savegame/slot01/save.dat` and `fallout.cfg`. Case 3 refuses
`MASTER.DAT` beside `master.dat`. Case 8 has no common folder and no
top-level archive, so both entries are a loose corpus (whose validation then
fails later, for the missing required members).

### 5.6 Keys and Storage Operations

`keys.tsv` gives 15 keys: 7 accepted (with their record keys) and 8 refused.
`old/notes.bak` is accepted in `mods`, where `.bak` is not reserved.

`storage-ops.tsv` is a 20-step script. Its formats: `op` is `commit`,
`read`, `list` or `slots`; a commit's `data` is a `;`-separated list of
`put KEY=VALUE` and `del KEY`, the value running from the first `=` to the
`;` or the end, as ASCII; `-` in `key` means "no prefix" and in `expect`
means an empty list. Selected steps:

- Step 5 is a Vanilla save over an enhanced one: the batch deletes
  `SLOT01/VAULTENG.TOML`, and step 6 finds it absent.
- Step 8's second key has the reserved suffix `.TMP`, so the whole batch is
  refused, and step 9 finds `SLOT02/SAVE.DAT` absent.
- Step 10 puts `SLOT03/SAVE.DAT` twice under two spellings; the later value,
  `T2`, wins.
- Step 14 lists the prefix `slot0`, which matches no whole component.
- Step 16: slots `SLOT01`, `SLOT03` and `QUICKSAV`. `SLOT02` has no
  `SAVE.DAT`, and `SLOT11` is not a slot, although its file is listed in
  step 20.

`quota.tsv` runs on a 20-byte quota. Step 1 uses 6 + 5 = 11 bytes. Step 2
would reach 11 + 6 + 6 = 23 and is refused. Step 3 replaces `a.toml` and adds
`b.toml`: 7 + 8 = 15. Step 5 deletes `a.toml` and adds `c.toml` in one batch:
8 + 12 = 20, equal to the quota, so it is accepted.

### 5.7 Mods

`mods-entries.tsv` holds six entries; `mods-expected.tsv` gives the results.
For the load order `mods/beta`, `mods/alpha`, `mods/gamma` (disabled),
discovery returns no manifest for `mods/beta` and the manifests of
`mods/alpha` and `mods/gamma`. `find_layers()` gives `mods/alpha`,
`mods/gamma` and `other`, but not the root-level `vault-mod.toml`. Mounting
`mods` as a layer serves every file below it, nested layers included, as a
folder layer would. Mounting `missing` is `LayerNotFound`.

### 5.8 The Compile Gate (Revision 2)

`wasm-gate.tsv` lists 11 gate decisions. Its inputs are `rustc_exit` (the probe's
exit status), `libdir` (`missing`, `empty` or `present`), `libcore_rlib` (`yes`,
`no`, or `-` when the directory was not looked in), `require` (the value of
`VAULT_REQUIRE_WASM`, or `unset`), and the results `wasm_check` and `host_check`
(`pass`, `fail:CRATE`, or `-` when not run). Its outputs are `outcome`, `exit`,
`first_line` and `last_line`.

- `dir-missing` is the owner's machine: the probe exits 0 and names a directory
  that does not exist. The gate skips, the host check passes, and it exits 0.
- `dir-empty` is a directory with no `libcore-*.rlib`: still not installed.
- `require-other` shows that only `1` requires the target: a value of `0` skips.
- `skip-host-fails` skips the target, but the host check fails, so the gate fails.
  Its first line is still the skip line.
- `required-missing` is a CI runner that lost its target: it fails, and runs
  nothing.
- `crate-fails` stops at `vault-formats`, the first failing crate.

`wasm-graph.tsv` gives each gated crate's workspace dependencies and its allowed
crates, as read on 2026-10-30 with `cap-std` removed from the graph. `vault-vm`
and `vault-formats` have the same six: `thiserror`, and what its derive macro
pulls in. `vault-core` adds `serde` and `slotmap`.

### 5.9 Slot Saves (Revision 3)

`slot-saves.tsv` lists 10 saves. `before` is a `;`-separated list of
`KEY=VALUE` entries in the `savegame` namespace, spelled as stored, and `image` a
`;`-separated list of `NAME=VALUE` files. `removed` lists the canonical keys that
the save removes, and `after` every `KEY=VALUE` entry afterward, by canonical key,
both comma-separated. `-` means none.

- `stale-map` is the rule itself: `DEN.SAV` is not in the image, so it goes.
- `any-case` compares by canonical key. The stored `arroyo.sav` is the image's
  `ARROYO.SAV`, so it is replaced. The stale `Den.Sav` goes.
- `sidecars-kept` removes `DEN.SAV` and `VAULTENG.TOML`, and keeps `VAULTSFL.DAT`
  and `NOTES.TXT`.
- `near-names` removes nothing: no other name there ends in `.SAV`.
- `unusual-stem` removes `OLD MAP.SAV`, which the loader's stem check would refuse.
- `other-slot` leaves `SLOT02/DEN.SAV` in place.

## 6. Decisions

Every open point of the brief is settled here.

- **D-1 (owner instruction).** An in-memory archive backend for
  `vault_formats::vfs` that mounts version 1 and version 2 archives and loose
  overrides from byte buffers, with no OS path, for browser drag-and-drop and
  embedded mobile bundles.
- **D-2 (owner instruction).** A portable `StorageBackend` trait in
  `vault_platform` for configuration, save slots and mod manifest discovery,
  shared by desktop filesystem I/O and browser or mobile key-value storage
  through one deterministic interface.
- **D-3 (owner instruction).** Hand-checkable synthetic fixtures in
  `tests/fixtures/spec023/`, every rule tagged, and this Decisions section.
- **D-4 (owner instruction, coordinator's notes).** SPEC-008 and SPEC-015
  are the archive sources, cited by section; the fixtures are tiny synthetic
  archives; modules are named by Rust path, and new interfaces are defined
  here.
- **D-5 (Team A default).** A memory archive applies exactly the file-backed
  reader's rules, safe rules included, and parity with the file-backed reader
  on the same bytes is the `[Must-match]`. The tags compare the engine with
  itself, as in SPEC-020.
- **D-6 (Team A default).** The profile selects the archive format, as
  SPEC-015 section 1 says. Bytes are probed only to word a refusal.
- **D-7 (Team A default).** A byte source is at most `2^32 - 1` bytes, since
  both formats use 32-bit offsets and sizes.
- **D-8 (Team A default).** Byte sources are shared and immutable, so an
  embedded bundle or a memory map can be mounted without copying, and stored
  members may be returned as sub-slices.
- **D-9 (Team A default).** Memory paths and storage keys use the loose
  provider's validator and canonical form. Duplicates and file-or-folder
  conflicts are refused, as a folder mount refuses case collisions.
- **D-10 (Team A default).** Drops: one common top folder is stripped;
  archives are recognized only at the top level, by name; with archives, the
  loose tree is the conventional `data/` folder without `data/savegame/`;
  without archives, the drop is a loose corpus; `patch000.dat` is ignored
  under `fallout1`; both required archives must be present.
- **D-11 (Team A default).** Game data is never written to a
  `StorageBackend`. A browser host supplies the archives each session (or
  caches them by its own means), because retail archives exceed the 64 MiB
  value limit and are the user's data, not the engine's state.
- **D-12 (Team A default).** Three namespaces, `config`, `savegame` and
  `mods`. The key-value record key is
  `vault-engine/1/<scope>/<namespace>/<canonical key>`, with a `shared`
  scope for `config` and `mods` and a per-profile scope for `savegame`, and
  a layout version so that a later layout cannot misread it.
- **D-13 (Team A default).** Keys keep their spelling and compare by their
  canonical form; listings return canonical keys in byte order. The
  filesystem keeps the spelling of the files it already has, so desktop
  slots keep their upper-case names.
- **D-14 (Team A default).** `.tmp` and `.bak` names are reserved in
  `config` and `savegame`, because the staged write uses them, and staging
  leftovers are never listed or read.
- **D-15 (Team A default).** A commit is an atomic batch. The filesystem
  backend uses the staged protocol the save path already uses, with its
  existing crash window; key-value hosts use their transactions.
- **D-16 (Team A default).** The trait is synchronous. Asynchronous stores
  preload the namespaces, apply commits to memory at once, and write them
  behind in order; `flush` reports durability, and a failed write blocks
  later commits until the store is reopened.
- **D-17 (Team A default).** `StorageBackend` lives in `vault_platform`, as
  the brief says, and `vault_core` cannot depend on it. The save-slot code
  therefore exchanges slot images, and the session moves them to and from
  storage.
- **D-18 (owner instruction, revision 3; a Team A default before that).** A save
  deletes `VAULTENG.TOML` when the new image has none, and every `.SAV` of the
  slot that the new image lacks, so old map files never leak into an overwritten
  slot. Every other file of the slot stays. Revisions 1 and 2 said a save deleted
  only `VAULTENG.TOML`, and that stale companions stayed "as today". That premise
  was wrong: the save code before this spec removed every `.SAV` of the slot, as
  the implementer reported. The loader still reads only the companions the
  manifest names.
- **D-19 (Team A default).** A layer is missing when it has no file, on every
  backend. A key-value store cannot hold an empty folder, so the desktop's
  empty folder is refused too, a `[May-improve]` change: an empty layer
  serves nothing.
- **D-20 (Team A default).** The reference backend's quota counts key and
  value bytes, so the quota fixture is hand-checkable. Hosts map their own
  quota errors to the same error kind.
- **D-21 (Team A default).** The names `ByteSource`, `MemoryDir`,
  `MemoryMount`, `classify_drop`, `from_bytes`, `StorageBackend`,
  `FsStorage`, `MemoryStorage`, `KvStorage`, `SlotImage` and the module
  `vault_platform::storage` are Team A's; an implementer may rename them.
- **D-22 (Team A default).** Under a memory mount, the settings file's path
  keys are unused and kept, so that the same file can move between a desktop
  and a browser.
- **D-23 (Team A default).** A default `host-fs` feature gates the
  file-backed code in `vault-formats`, so that the memory path is proven to
  build without it.
- **D-24 (Team A default).** `config` keys are single `.toml` names, so that
  listing the desktop's settings folder never returns unrelated files.
- **D-25 (Team A default).** The vectors are Team A's, computed from this
  spec, and live in `tests/fixtures/spec023/`, not in `oracle/`, which is
  reserved for research-side vectors.

Revision 2 adds:

- **D-26 (Team A default).** The owner's brief of 2026-10-30 asks to author this
  spec, but revision 1 was approved on 2026-10-27. It is not rewritten, because
  its rules already meet the brief. This is revision 2. Item by item:
  - **Byte-buffer archive backend:** revision 1, sections 2.1 to 3.6 (D-1).
    Version 1 and version 2 archives and loose files mount from byte sources,
    with no OS path, through the loose provider's path rules and SPEC-008's
    selected-database layering.
  - **`StorageBackend`:** revision 1, sections 3.7 to 3.15 (D-2). Settings, save
    slots and mod discovery; atomic commits (section 3.8); listing (section 3.7);
    the 64 MiB value limit and a quota (sections 2.4 and 3.8); a closed error type
    (section 3.7); `FsStorage` and `MemoryStorage` (sections 3.9 and 3.14).
  - **The compile gate:** new, section 3.17.
- **D-27 (owner instruction).** `vault-formats`, `vault-vm`, `vault-core` and
  `vault-guidance` pass a `wasm32-unknown-unknown` compile check in `xtask` and
  in CI, with zero new dependencies.
- **D-28 (Team A default).** The dependency listing of 2026-10-30 shows that
  `cap-std` is unconditional in `vault-formats`, and that `vault-core` and
  `vault-guidance` take default features. So the gate needs section 3.6's
  optional dependency and forwarding, which revision 1 did not state. They change
  no locked package, so they keep the owner's "zero new dependencies".
- **D-29 (Team A default).** Gated crates branch on the `host-fs` feature, not on
  the target, so that the host's `--no-default-features` tests run the code the
  target compiles.
- **D-30 (owner instruction for a graceful skip; Team A default for the probe).**
  Skip only when the target's standard library is missing. The probe tests the
  directory and a `libcore-*.rlib` in it, because `rustc` prints the path whether
  or not it exists (measured by Team A). It does not use `rustup`, so it works
  with a distribution's toolchain, and it needs no network.
- **D-31 (Team A default).** `VAULT_REQUIRE_WASM=1` makes a missing target a
  failure, as `VAULT_REQUIRE_GPU=1` does for an adapter, and the CI job sets it, so
  that CI can never pass by skipping.
- **D-32 (Team A default).** A skip still runs the four checks on the host. A
  skipped machine then catches a broken feature gate, which is the likeliest way
  for these crates to drift.
- **D-33 (Team A default).** Only a missing target skips. With the target
  installed, every failure fails, a download included, so that a skip never hides
  a broken build.
- **D-34 (Team A default).** Because the gate only compiles, the calls that
  compile but fail at run time on this target are ruled out by section 3.17's
  list, and clippy is the recommended enforcement.
- **D-35 (Team A default).** The gate lands after section 3.6, in the order of
  section 3.18, because it fails until then.
- **D-36 (Team A default).** The names `cargo xtask wasm-check`,
  `VAULT_REQUIRE_WASM` and the CI job `wasm-check` are Team A's. `cargo xtask
  check` gains the gate as a fourth step. `AGENTS.md` and `docs/REFERENCE.md` say
  three today, and the implementer updates them, because this spec's author does
  not change those files.

Revision 3 adds these decisions.

- **D-37 (Team A default).** The cleanup goes in the save's own commit, so a
  failed save restores the stale files with everything else, and in a quota the
  deletes count toward the batch's net effect. It lists the slot first because
  `commit` has no wildcard. D-18 says what it does.
- **D-38 (owner instruction for the sweep; Team A default for the rest).** The
  sweep touches only `.SAV` names, so unknown sidecars such as `VAULTSFL.DAT` stay.
  SPEC-024b section 5.8 keeps its own named delete of that one file on a save with
  sfall mode off. That delete is not part of this sweep. Without it, a slot just
  saved with sfall mode off would hold an old sidecar, and that spec's loading
  table would refuse the slot. Until SPEC-024b is implemented, nothing deletes the
  file.
- **D-39 (Team A default).** `.SAV` is matched after case folding and by name
  alone, as the code before this spec matched every `.SAV` of the slot. The
  loader's stem check is not used, so an unloadable leftover cannot stay forever.

## 7. Clean-Room Verification Checklist

- [x] Zero C/C++ code snippets or struct definitions included.
- [x] No implementation-source file path (the `CLAUDE.md` ingestion rule),
  and no 40-character hexadecimal string. Engine code is named only by Rust
  module path and logical type name.
- [x] Every multi-byte integer's width and endianness stated: version 1
  archives big-endian (SPEC-008), version 2 archives little-endian with a
  big-endian Adler-32 (SPEC-015).
- [x] Every rule tagged `[Must-match]`, `[May-improve]` or `[Safe rule]`;
  tables are tagged once and their rows inherit the tag.
- [x] Synthetic test vectors only; no archive, member or save holds game
  data. Every archive offset in section 5 was checked by hand.
- [x] Decisions separate owner instructions from Team A defaults.
- [x] Revision 2: `wasm-gate.tsv` was written from section 3.17 by a throwaway
  script, and a second, separate script recomputed every row's outcome, exit and
  lines from the rule. `wasm-graph.tsv` holds crate names as `cargo tree` printed
  them, with `cap-std` pruned, and each name was confirmed to be in the host
  graph.
- [x] Revision 2 keeps the ingestion rule: no implementation-source path, no
  40-character hexadecimal string, and no code listing. No engine Rust code and no
  crate manifest was opened. The only inputs from the workspace are the dependency
  listing and the one `rustc` measurement, both disclosed in the provenance.
- [x] Revision 2 changes no revision 1 rule or fixture. Sections 3.6 and 3.17 add
  to them.
- [x] Revision 3: `slot-saves.tsv` was written from section 3.11 by a throwaway
  script, and a second, separate script recomputed every row from that rule. No
  existing fixture changed.
- [x] Revision 3 keeps the ingestion rule: no implementation-source path, no
  40-character hexadecimal string, no code listing, and no engine Rust code read.
