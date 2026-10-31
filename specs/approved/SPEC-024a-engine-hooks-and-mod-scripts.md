# SPEC-024a: Engine Hooks and Mod Scripts

**Status:** approved by the Spec-Author sub-agent (Team A), 2026-10-31, under AGENTS.md "Spec Authoring (Team A)". Revision 1.

**Provenance.** Author: the Spec-Author sub-agent (Team A), 2026-10-31. Sources:

- the owner's brief for this spec, 2026-10-31: one engine hook interface for sfall's `hs_*.int` scripts and for mods declared in `vault-mod.toml`; how a manifest attaches `.INT` scripts and how global scripts load; order and determinism; a reserved word range with its first words; and three exclusions;
- the project's approved specs: SPEC-004 (the VM, host-initiated calls), SPEC-005 (registries, section 3.8's player art), SPEC-017 (word sets, the stack model), SPEC-020 (the session), SPEC-021 (hashes, the headless step, checkpoints), SPEC-023 (storage namespaces), and SPEC-024b with `oracle/SPEC-024/` (the hook list and its boundaries);
- `docs/REFERENCE.md`: the mod manifest grammar, what a replay reads, the input-log records, and the known gaps of sfall mode;
- black-box runs of the built tools on synthetic data: `vault-platform --help`, to see which flags a replay has, and `asset-audit --dump-vm-trace` on a copy of the fixture program.

No decompilation, disassembly, re-implementation source, game asset, research-vault material, or engine Rust code was used. No sfall material was consulted: everything said here about sfall's words and hooks comes from SPEC-024b and its oracle. Team A's general knowledge comes from model training data that cannot be audited (`PROVENANCE.md`).

**Fixtures:** `tests/fixtures/spec024a/`, written by Team A from this spec. They are not in `oracle/`, which is reserved for research-side vectors.

## Revision history

- **Revision 1 (2026-10-31, Team A).** First text. It takes up SPEC-024b Decision D-21, which said section 4.9's global-script rules yield to this document.

## Clean-Room Rules for Spec Authors

- Document only binary layouts, grammars, word numbers, stack transitions, ordering rules, and observable behavior.
- Do not copy code, struct definitions, function signatures, or internal variable names from any decompilation or re-implementation.
- Public SSL names, public hook names, and public script file names are permitted, as in SPEC-024b.
- Name engine modules by Rust path only.
- Provide synthetic test vectors.

## 1. Overview & Scope

The engine gets one hook interface. A **hook point** is a named place where the engine asks scripts to look at, and perhaps change, a value it has just worked out. Two kinds of mod attach to the same points:

- **sfall scripts**: `hs_*.int` files and procedures that scripts register, as SPEC-024b defines them;
- **manifest mods**: `.INT` scripts that a layer's `vault-mod.toml` declares.

This spec defines:

1. when manifest scripting is on, and the manifest grammar (section 2);
2. the hook points as one list (section 3);
3. how scripts attach, and how global scripts load (section 4);
4. the order of a chain, how returns combine, and what an error does (section 5);
5. the determinism rules (section 6);
6. the reserved word range and its first ten words (section 7).

Section 8 records what stays out of scope. The chain and the registry stay in `vault_core::host`, the words in `vault_vm::extensions`, and manifests in `vault_platform::mods`.

**Compatibility tags.** The original game has no such interface, so in this spec the tags compare an implementation with **this spec and its fixtures**, as in SPEC-021.

| Tag | Meaning here |
| --- | --- |
| `[Must-match]` | The word numbers, the grammar, the order, and the vector results must be reproduced exactly |
| `[May-improve]` | An implementation freedom, allowed only if no result of this spec changes |
| `[Safe rule]` | How the engine handles a state this spec does not want: it refuses, ignores, or clamps as stated, and never panics or reads out of bounds |

A tag covers its own table row, bullet, or sentence. A table or list with no tag of its own takes the tag of the sentence that introduces it. A heading statement such as "all of this section" covers everything under that heading unless a line says otherwise. Sentences that give a reason are not requirements. `[Must-match]`

**Invariants.** `[Must-match]`

1. With no scripted mod (section 2.1) enabled, everything is as it was without this spec: no word, file, record, or chain below exists. SPEC-024b's sfall mode behaves as SPEC-024b says.
2. With sfall mode on and no scripted mod, a chain is exactly SPEC-024b's chain.
3. A scripted mod whose members write nothing changes no formula.

**Notation.** `arg0` and `ret0` are as in SPEC-024b. "Lowest layer" means first in the load order. "Member" means one attached procedure in a chain.

### 1.1 What this changes in other specs

These amendments apply only while manifest scripting is on. SPEC-024b and SPEC-017 keep their text, and this table states the difference.

| Text | Change |
| --- | --- |
| SPEC-024b section 4.9, global scripts | Yields to section 4.3, by SPEC-024b's Decision D-21. Its rules still hold for `gl*` scripts under sfall mode |
| SPEC-024b section 6.1, setup | Gains step 4 (section 4.4) |
| SPEC-024b section 6.2, `register_hook_proc_spec` | "The end of the chain" means the end of the sfall block (section 5.1) |
| SPEC-024b section 6.2, who may register | A manifest script counts as a global script |
| SPEC-024b section 6.19 and Decision D-24 | `HOOK_ADJUSTFID` (26) has boundary text here, for the mod block only. Its sfall block stays inert, as those sections say (Decision D-17) |
| SPEC-024b section 1, invariant | With sfall mode off, a hook point can run for manifest members. `hs_*.int`, sfall's words, arrays, and the sidecar still do not exist |
| SPEC-017 section 2.1 and SPEC-024b section 3.2 | Dispatch indices 768 to 777 are registered slots (section 7.1). Every other index those sections call unregistered stays so |

## 2. Manifests and Activation

### 2.1 Manifest scripting

A layer is **scripted** when its `vault-mod.toml` has at least one `[[hooks]]` or `[[global_scripts]]` row. `[Must-match]`

- **Manifest scripting** is on exactly when at least one enabled layer is scripted. Enabling the layer is the opt-in, and there is no separate switch. `[Safe rule]`
- It is fixed when the session is built. Loading a save does not change it. `[Safe rule]`
- It works under `fallout1` and `fallout2`, under either preset, and with sfall mode on or off. `[Safe rule]`
- A disabled layer counts for nothing, and its rows are not read for conflicts. `[Must-match]`

### 2.2 Manifest additions

`vault-mod.toml` keeps its grammar and its rule that an unknown key or table is an error. These keys and tables join it. `[Must-match]`

```toml
name = "Cheap AP"
id = "cheap-ap"
api = 1

[settings]
ap_cost = 1

[[global_scripts]]
file = "gl_cheapap.int"
repeat = 0
type = 0

[[hooks]]
point = "HOOK_CALCAPCOST"
file = "gl_cheapap.int"
procedure = "ap_cost"
```

| Key | Where | Required | Rule |
| --- | --- | --- | --- |
| `id` | Top level | With any table below | `[a-z0-9_-]`, 1 to 32 bytes. Unique across enabled layers |
| `api` | Top level | With any table below | Decimal integer. The word API the mod needs: 1 in this revision |
| `<key>` | `[settings]` | No | Section 7.4 |
| `file` | `[[global_scripts]]`, `[[hooks]]` | Yes | A base name under `scripts/`: `[A-Za-z0-9_-]`, 1 to 32 bytes, then `.int` in any case |
| `repeat` | `[[global_scripts]]` | No, default 0 | Decimal integer, at least 0. As `set_global_script_repeat`: runs `start` every n steps, and 0 never |
| `type` | `[[global_scripts]]` | No, default 0 | 0 to 3, as `set_global_script_type` |
| `point` | `[[hooks]]` | Yes | A `hook_name` of `tests/fixtures/spec024a/hook-points.tsv`, as exact bytes |
| `procedure` | `[[hooks]]` | No, default `start` | `[A-Za-z0-9_]`, 1 to 31 bytes |

Each of these stops the launch, with `mod_list:` and the folder's name, as a malformed manifest does today. The launcher shows the same error on the layer. `[Safe rule]`

- A key or row outside the table above, or a value outside its rule.
- `api` above 1, so a mod never runs against an engine that lacks its words.
- A `file` absent from `scripts/` in the manifest's own layer.
- A `file` whose lowercased name begins with `hs_`. Those names belong to SPEC-024b's discovery.
- Two rows of one manifest with the same `file` and `point`, or two `[[global_scripts]]` rows with the same `file`.
- Two enabled layers with the same `id`, or naming the same `file` (compared lowercased). The message names both, as the existing conflicts do.

A `file` is read through the `Vfs` layers, so a higher layer may replace it, and that is an ordinary file override. The row still belongs to the mod that declared it. Every script passes the mod-script check of `docs/REFERENCE.md`, with section 7's words in its table. `[Must-match]`

A manifest with `[settings]` and no script row is valid, is not scripted, and has no effect. `[Safe rule]`

### 2.3 Replays

A replay reads neither `vault-engine.toml` nor a manifest's `[enhanced]` and `[scripting]` keys, and that stays true. It therefore needs to be told the layers. `[Safe rule]`

- `--mod-list PATH` names a load order, as a `mod_list` file does. It wins over the settings file in a live session, and it is how a replay mounts layers.
- A session with manifest scripting on writes one `mod_stack` record to its input log, after the leading records `docs/REFERENCE.md` lists and before the first `tick`:

```json
{"command_id":7,"kind":"mod_stack","api":1,"mods":[{"id":"cheap-ap","version":"1.0.0","manifest":"EE7C38513A262548","settings":{"ap_cost":"i:1"}}]}
```

| Field | Content |
| --- | --- |
| `api` | The engine's word API |
| `mods` | Every scripted enabled layer, lowest first |
| `id`, `version` | From the manifest; `version` is `""` when absent |
| `manifest` | `fnv1a64` (SPEC-021 section 2.1) of the manifest's bytes, 16 uppercase hexadecimal digits |
| `settings` | Every declared key's value at session start, ascending by key bytes: `i:` and decimal, `f:` and the eight uppercase hexadecimal digits of the binary32 bits, or `s:` and the text |

- A replay compares its scripted layers with the record: the same `id`s in the same order, with the same `version` and `manifest`. A difference is refused before the first step, as a usage error, with `mod stack mismatch:` and the first `id` that differs. A log with no record means no scripted layer.
- A replay takes every setting from the record and reads no settings file. Its `mod_set_setting` writes change the session's table and commit nothing.
- The record exists only with manifest scripting on, so no existing log or digest changes.

`input_index` counts every line of a log (SPEC-021 section 3.10), so this record moves it by one against a log without the record. Two runs of one log still give identical streams. `[Must-match]`

## 3. Hook Points

### 3.1 The list

This is the one list. `tests/fixtures/spec024a/hook-points.tsv` holds it with all 51 rows of `sfall-hooks.tsv`. IDs and names are SPEC-024b's. `[Must-match]`

The Boundary column says where each point's exact text is. SPEC-024b's sections stay normative for their points, and if this table and those sections ever differ, those sections win. `[Must-match]`

| ID | Hook | Point | Arguments | Returns | Native result it can override | Boundary |
| ---: | --- | --- | --- | --- | --- | --- |
| 0 | `HOOK_TOHIT` | To-hit | 0 chance after the cap; 1 attacker; 2 target; 3 body part; 4 source hex; 5 attack type; 6 range-aware; 7 chance before the cap | ret0 chance, -99 to 999 | The hit chance | SPEC-024b 6.8 |
| 1 | `HOOK_AFTERHITROLL` | After the hit roll | 0 roll; 1 attacker; 2 target; 3 body part; 4 hit chance | ret0 roll, 0 to 3; ret1 body part, 0 to 8; ret2 a live critter | The roll, the body part, the target | SPEC-024b 6.9 |
| 2 | `HOOK_CALCAPCOST` | AP cost | 0 critter; 1 attack type; 2 aimed; 3 default cost; 4 weapon | ret0 cost; negative becomes 0 | An item action's AP cost | SPEC-024b 6.10 |
| 5 | `HOOK_COMBATDAMAGE` | Damage | 0 target; 1 attacker; 2 target damage; 3 attacker damage; 4 target flags; 5 attacker flags; 6 weapon; 7 body part; 8 multiplier; 9 rounds; 10 knockback; 11 attack type; 12 int 0 | ret0 target damage; ret1 attacker damage; ret2 target flags; ret3 attacker flags; ret4 knockback | Damage and special-effect flags of target and attacker, and knockback | SPEC-024b 6.11 |
| 8 | `HOOK_USEOBJON` | Use an item on a critter | 0 target; 1 user; 2 item | ret0 0 keep, 1 destroy, -1 or unset engine | Whether the use proceeds, and whether the item is destroyed | SPEC-024b 6.15 |
| 10 | `HOOK_BARTERPRICE` | Barter price | 0 player side; 1 other side; 2 price; 3 merchant offer table; 4 caps on it; 5 its value; 6 player offer table; 7 its value; 8 offer pressed; 9 party member | ret0 price, -1 unchanged; ret1 offer value | Price, and the value of the player's offer | SPEC-024b 6.12 |
| 11 | `HOOK_MOVECOST` | Movement cost | 0 critter; 1 hexes; 2 original cost | ret0 cost; negative becomes 0 | The AP cost of the hexes | SPEC-024b 6.13 |
| 18 | `HOOK_USEOBJ` | Use an object | 0 user; 1 object | ret0 as `HOOK_USEOBJON` | Whether the use proceeds, and whether the item is destroyed | SPEC-024b 6.15 |
| 19 | `HOOK_KEYPRESS` | Key press | 0 pressed; 1 scan code; 2 virtual-key code | ret0 scan code; 0 no override | The logical key the game sees | SPEC-024b 6.13 |
| 24 | `HOOK_INVENTORYMOVE` | Inventory move | 0 target; 1 item; 2 item replaced, or 0 | ret0 -1 or unset engine; any other value prevents | Whether the move is allowed | SPEC-024b 6.13 |
| 26 | `HOOK_ADJUSTFID` | Art change | 0 art ID worked out; 1 art ID stored now | ret0 art ID | The art ID stored on the player object | Section 3.3, mod block only |
| 29 | `HOOK_SETGLOBALVAR` | Global variable write | 0 index; 1 value | ret0 value | The value stored | SPEC-024b 6.18 |
| 31 | `HOOK_GAMEMODECHANGE` | Game mode change | 0 exiting; 1 previous mode | None | None: a notice | SPEC-024b 6.14 |
| 40 | `HOOK_STDPROCEDURE` | Standard procedure | 0 event; 1 owner; 2 caller; 3 int 0; 4 object acted on; 5 fixed parameter | ret0 -1 cancels | Whether the procedure runs | SPEC-024b 6.16 |
| 43 | `HOOK_ENCOUNTER` | Encounter | 0 event type; 1 map; 2 Special entry; 3 table; 4 entry | ret0 map, -1 cancels event 0; ret1 1 loads with no groups | The map that loads, or whether the encounter happens | SPEC-024b 6.17 |

### 3.2 Rules for every point

- SPEC-024b sections 6.4 to 6.7 apply to every point, whichever kind of mod a member belongs to: the context, the composition rules, the phases and previews, and the numbering. `[Must-match]`
- Under `fallout1`, `HOOK_ENCOUNTER` has no boundary, because its text anchors on SPEC-018. A row or registration for it is kept and never runs. Every other point fires under both profiles. `[Safe rule]`
- With sfall mode off, a preview query still runs (SPEC-024b section 6.6). So does the once-per-step comparison of `HOOK_GAMEMODECHANGE`, on the same bit set as `get_game_mode` (section 7.3). `[Safe rule]`
- An empty chain costs nothing and changes nothing. `[Must-match]`

### 3.3 `HOOK_ADJUSTFID` (26, `hs_adjustfid.int`): art change

SPEC-024b gives this point numbers and types only. The text below is Team A's, from SPEC-005 section 3.8, and is not a claim about sfall (Decision D-17). All of it is `[Safe rule]`.

**Mod block only.** This point runs its mod block and skips its sfall block. So `hs_adjustfid.int` and every sfall registration on ID 26 load and register and run nothing, as SPEC-024b section 6.19 says, until the research side gives that block its boundary. One Et Tu script uses this hook (`oracle/SPEC-024/mod-opcode-usage.tsv`), written to a meaning this spec does not know.

**Boundary.** Runs each time one of SPEC-005 section 3.8's rules is about to replace the **player object's** art, after the rule has worked out the new art ID and before it is stored or registered:

1. the gender update, at its step 2;
2. the inventory window's close, when it applies the figure art;
3. `wield_obj_critter` with armor, at its step 3, before the art change is registered;
4. `rm_obj_from_inven` taking worn armor off the player;
5. `refresh_pc_art` (SPEC-024b section 4.11).

It does not run for any other critter, or when the rule leaves the art unchanged, such as the gender update's step 3.

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | int | The art ID the rule worked out |
| arg1 | int | The art ID the player object holds now |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | The art ID the rule stores or registers | Must be a critter-category art ID the SPEC-007 codec accepts. Anything else is ignored, with a diagnostic |

- The player prototype's art, at the gender update's step 4, is not overridden.
- The open inventory window's figure stays unhooked. It is drawn while the window is open, and SPEC-005 section 3.8 says the object's art does not change until the window closes. The point runs at the close, in the Input or Simulation phase.

### 3.4 The other points

The other 35 rows have no boundary text. Their scripts load and register, and no chain runs, as SPEC-024b section 6.19 says. A manifest row may name one: it is kept, runs nothing, and writes one diagnostic at setup. IDs 49 to 60 are reserved. `[Must-match]`

## 4. Attaching Scripts

### 4.1 Mods and owners

A manifest script's **owner** is the mod whose manifest declares it. A program's owner is fixed when it loads. A program no manifest declares has no owner, even when a mod's layer served its file. `[Must-match]`

A manifest script is one program instance for the session, whichever rows name it. It outlives a map and keeps its state between runs, as a hook script does (SPEC-024b section 6.1). `[Must-match]`

### 4.2 Hook rows and registration

There are three routes onto a point. `[Must-match]`

| Route | Who | Block (section 5.1) |
| --- | --- | --- |
| A `[[hooks]]` row | A manifest | Mod block |
| `mod_register_hook` (section 7.3) | A program with an owner | Mod block |
| `register_hook`, `register_hook_proc`, `register_hook_proc_spec` | Any global or hook script, under sfall mode | Sfall block, by SPEC-024b section 6.2 |

- The word chooses the block, not the script. So an sfall mod packaged with a manifest keeps its SPEC-024b order. `[Must-match]`
- A `[[hooks]]` row names a procedure, and the host finds it by SPEC-004 section 3.7's name lookup. An absent name leaves the row inert, with one diagnostic at setup. The fallback to procedure index 1 does not apply to a member. `[Safe rule]`
- Inside the mod block, a program has at most one registration per point. Registering again changes its procedure and keeps its position. Procedure 0 removes it, a manifest row's included, until the next setup. `[Safe rule]`
- `mod_register_hook` from a program with no owner is ignored, with a diagnostic. A hook ID without a row in `hook-points.tsv`, or in 49 to 60, is ignored, with a diagnostic. `[Safe rule]`
- Registration is not saved. Setup rebuilds it (section 4.4). `[Must-match]`

### 4.3 Global scripts

This section is what SPEC-024b section 4.9 yields to.

**What a global script is.** `[Must-match]`

- Under sfall mode, a `.INT` under `scripts/` whose base name begins with `gl`, without case, as SPEC-024b says.
- With manifest scripting on, a file a `[[global_scripts]]` row names, whatever its name.
- A file that matches both is a manifest script only. It loads once and runs once, in the manifest's place. `[Safe rule]`

A global script has no attached object. `[Must-match]`

**Order.** Wherever global scripts run in turn, sfall's come first, ascending by lowercased base name, compared as bytes. Then come manifest global scripts, lowest layer first, each layer in row order. `[Must-match]`

**Events.** `[Must-match]`

- `start` runs once in setup (section 4.4).
- `map_enter_p_proc`, `map_exit_p_proc`, and `map_update_p_proc` run at those events, in that order. A script without the procedure is skipped, with no fallback.
- `repeat` and `type` work as `set_global_script_repeat` and `set_global_script_type` do, and those words change them under sfall mode. Repeating scripts run in phase 2 of the step, in that order. `[Safe rule]`

A repeating script's `start` is called by the host, so it must leave a result or placeholder, as SPEC-004 section 3.6 requires of any host-called procedure. `[Must-match]`

### 4.4 Session setup

SPEC-024b section 6.1's setup gains one step, and runs when sfall mode or manifest scripting is on. It runs after a new game or a slot load finishes, and before the first phase 2 that follows. `[Must-match]`

1. Clear the hook registry.
2. Under sfall mode: for each hook ID ascending, load its `hs_` file if it exists and run `start`, with `init_hook` giving 1.
3. Under sfall mode: for each sfall global script, in order, run `start`.
4. With manifest scripting on, for each scripted enabled layer, lowest first:
   1. attach its `[[hooks]]` rows, in row order;
   2. load each script its `[[global_scripts]]` rows name, in row order, then each other script its `[[hooks]]` rows name, in the order of first appearance. Loading one runs its `start`, with `init_hook` giving 1.

A row's position comes from its order in the manifest, not from when its program loaded. A word registration in a `start` falls after every row of its mod (section 5.1). `[Must-match]`

A script the mod-script check refuses takes SPEC-006 section 3.7's cannot-load path, and none of its rows runs until the next setup. `[Safe rule]`

## 5. Order and Composition

### 5.1 The chain

One point has one chain, in this order: `[Must-match]`

| Block | Members | Order |
| --- | --- | --- |
| Sfall | Everything SPEC-024b section 6.3 lists | `register_hook` and `register_hook_proc`, newest first; the `hs_` script's `start`; `register_hook_proc_spec`, oldest first |
| Mod | `[[hooks]]` rows and `mod_register_hook` registrations | Lowest layer first. Inside a layer, rows in manifest order, then word registrations in call order |

Sfall scripts run first, so manifest mods have the last word, and the highest layer has the last word of all. That matches a layer winning a file or a key. There is no priority key (Decision D-9). `[Must-match]`

With sfall mode off, the sfall block is empty. With manifest scripting off, the mod block is empty. `[Must-match]`

### 5.2 How returns combine

SPEC-024b section 6.5 applies across both blocks with no change. `[Must-match]`

- Each member starts with its argument and return cursors at 0.
- `set_sfall_arg` and `mod_set_hook_arg` change what later members read.
- A return stays set until a later member writes that index, so the last writer wins, index by index.
- Only returns reach the engine. An unset return leaves the native value.
- The point's limits (section 3.1) apply once, to the final value, after the last member.

Both word families work on one context. A return one family writes is a return the other can overwrite. Writing by index moves no cursor. `[Must-match]`

### 5.3 Errors

`[Must-match]`, from SPEC-024b section 6.5 and its Decision D-19, for both blocks.

- A member that ends in a VM error leaves **every** chain of both blocks until the next setup. Its writes up to that point stay, and the chain goes on with the next member.
- That covers the 100,000-instruction limit of a host call (`docs/REFERENCE.md`): running out of it is a VM error.
- A member of a program that ended in a VM error anywhere else, such as a repeating global script, has left its chains the same way.
- A word that would suspend a member ends that member, with a diagnostic. Its writes stay. It is not unloaded, and it runs on the next query.
- A return that cannot convert leaves that index as it was, with a diagnostic. It does not undo an earlier member's write (Decision D-26).
- One mod's failure never stops the session or another mod. No failure above changes the stack contract of the word that caused the chain.

### 5.4 Depth and re-entry

- **Depth.** One counter covers all chains, of either block and any point. Past **8 nested chains**, a chain is skipped with a diagnostic and the native values stand. `[Must-match]`, from SPEC-024b Decision D-20.
- **Membership.** A chain's members are fixed when its query starts. A registration made during a chain counts from the next query. A member that left its chains during this one is skipped when its turn comes. `[Safe rule]`
- **Re-entry.** A member whose program is already running on an outer chain is skipped with a diagnostic, and the chain goes on. The one exception is SPEC-024b section 6.18: a member reached from that program's own `set_global_var` runs. This is the rule `docs/REFERENCE.md` records under "Known gaps". `[Safe rule]`

## 6. Determinism

All of this section is `[Must-match]`.

1. **Phases.** A chain runs only in phase 1 (Input), in phase 2 (Simulation), or in setup. It never runs in phase 3 (Checkpoint) or phase 4 (Presentation), so hooks never run while drawing. A value that is only displayed comes from a preview query in phase 2 (SPEC-024b section 6.6).
2. **Close of a session.** `signal_close_game`'s `HOOK_GAMEMODECHANGE` run is the last thing in phase 2 of that step, so it obeys rule 1 (Decision D-19).
3. **Once per query.** A chain runs once per engine query, and queries come in the host specs' order. A member's random draws are part of the deterministic sequence.
4. **Order comes from data.** Chain order comes from the load order's list, a manifest's row order, and byte comparisons of lowercased names. It never comes from a folder listing, a hash map's iteration, an address, a clock, or a locale.
5. **Hashes.** `state_hash` stays `VSH1`, `combat_hash` stays `VCH1`, `sfall_hash` stays `VSX1`, and the checkpoint format stays version 1. This spec adds no covered value. A member's effect reaches a hash only through covered state it changes, as any other script's does.
6. **No new sources of difference.** The hook registry, mod settings, the mod log, a program's stacks and owner, and the `mod_stack` record are outside every stream, as SPEC-021 section 3.1 leaves the VM.
7. **Inputs.** A scripted run depends on the layers and on the setting values. Layers are compared and values taken from the `mod_stack` record (section 2.3), so a replay cannot silently run a different stack.
8. **Bounds count instructions.** The instruction limit and the depth limit count events, not time, so they fall at the same place on every host.
9. **Words.** No word of section 7 reads a clock, a host window, a locale, or a file during a step.
10. **Saves.** `SAVE.DAT`, `VAULTENG.TOML`, and `VAULTSFL.DAT` do not change. No slot records a mod list, and loading a slot needs no particular mod (Decision D-15).

So two runs of one log, with the same layers, give identical checkpoint streams on every platform. A mod whose members write nothing and draw nothing leaves the `state_hash` and `combat_hash` columns as they are with no mod.

## 7. Extension Words

### 7.1 The reserved range

**`0x8300..=0x83FF` is reserved for this spec.** `[Must-match]`

| Range | Owner | Under this spec |
| --- | --- | --- |
| `0x8000..=0x8183` | SPEC-004, SPEC-005, SPEC-017 | Unchanged |
| `0x8156..=0x8281` | SPEC-024b, under sfall mode | Unchanged |
| `0x8200..=0x82FF` | The enhanced range (SPEC-017 section 2.2) | Unchanged |
| `0x8300..=0x83FF` | **This spec** | New |
| `0x8400` and up | None | Bits 10 to 14 set: refused at load unless a slot-1 push (SPEC-004 section 2.5), unchanged |

It collides with neither spec: SPEC-017 stops at `0x82FF`, and `sfall-opcodes.tsv` stops at `0x8281`. The words dispatch on the low ten bits, as SPEC-004 section 3.8 requires, as indices 768 to 777.

- `0x8300..=0x8309` are in the set only with manifest scripting on, under either profile and with sfall mode on or off. Otherwise a program containing one is refused at load, as today. `[Must-match]`
- `0x830A..=0x83FF` are refused at load in every session. A script using a word this engine does not have takes the mod-script check's refused path, rather than failing partway through. `[Safe rule]`
- `opcode_exists` gives 0 for these words, because it reports P1 rows of SPEC-024b only. `[Safe rule]`
- `engine_api_version` (`0x8200`) stays as it is. This range has its own version. `[Safe rule]`

### 7.2 Operands

SPEC-024b section 4.1 applies to these words whether sfall mode is on or off: its value model, its operand classes, and its failed-operand rule. A failed operand pops every operand, has no effect, writes a diagnostic, and pushes the default. It is not a VM error. Stack underflow stays a VM error. `[Safe rule]`

The mod-script check's stack model (SPEC-017 section 8.2) reads the `pops` and `push` columns of `tests/fixtures/spec024a/mod-words.tsv`. `[Must-match]`

### 7.3 The words

Numbers, pop order, and pushes are `[Must-match]`. Pop order is first popped to last popped.

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x8300` | `mod_api_version` | none | I | 1 |
| `0x8301` | `mod_get_setting` | default A, key S | A | The caller's mod's setting, or the default (section 7.4) |
| `0x8302` | `mod_set_setting` | value A, key S | none | Writes the caller's mod's setting (section 7.4) |
| `0x8303` | `mod_log` | text S | none | One line in the mod log (section 7.5) |
| `0x8304` | `mod_register_hook` | procedure P, hook I | none | Registers the procedure in the mod block (section 4.2) |
| `0x8305` | `mod_hook_id` | none | I | The innermost chain's hook ID, or -1 outside a chain |
| `0x8306` | `mod_get_hook_arg` | index I | A | That argument, or int 0 out of range or outside a chain. It moves no cursor |
| `0x8307` | `mod_set_hook_arg` | value A, index I | none | As `set_sfall_arg`. Ignored out of range or outside a chain |
| `0x8308` | `mod_set_hook_return` | value A, index I | none | Sets return `index`, converted by the point's return type. Ignored out of range or outside a chain |
| `0x8309` | `mod_game_mode` | none | I | The bit set of SPEC-024b section 4.4 |

The words give a manifest mod what `get_sfall_arg_at`, `set_sfall_arg`, `set_sfall_return`, and `get_game_mode` give an sfall script, because those do not exist with sfall mode off. Under `fallout1`, the bits for screens that profile lacks are never set. `[Safe rule]`

### 7.4 Settings

A mod's settings are its `[settings]` keys. All of this section is `[Safe rule]`.

- **Keys.** `[A-Za-z0-9_]`, 1 to 31 bytes, case-sensitive, at most 64 per mod.
- **Values and types.** A TOML integer is int, a decimal with a fraction or exponent is float, and a quoted string is string, of at most 255 bytes. `true` and `false` are ints 1 and 0. A decimal rounds to the nearest binary32, ties to even. Another value is a manifest error. A key's type never changes.
- **Reading.** `mod_get_setting` gives the value. An undeclared key, or a caller with no owner, gives the default operand, with one diagnostic per program. A key that is not a string is a failed operand and pushes int 0.
- **Writing.** `mod_set_setting` takes effect at once. The value converts to the key's type: int by class `I`, float by class `F`, and string only from a string. A value that cannot convert, an undeclared key, or a caller with no owner changes nothing, with one diagnostic.
- **Another mod's settings** cannot be read or written.
- **Storage.** A key changed from its manifest default is kept in the `config` namespace (SPEC-023 section 2.4) under `mod-settings-<id>.toml`, as `key = value` lines ascending by key bytes, with the manifest's value grammar.
- **Session start.** Each key takes its stored value, else its manifest default. A stored key the manifest does not declare, or a value of the wrong type, is dropped, with a diagnostic.
- **Commit.** At the end of phase 2 of a step that changed a setting, each changed mod's file is written in one batch. A failed commit is a diagnostic. The session's table stays as written, and nothing about the world changes. A replay never commits.
- **New game and load.** Neither changes a setting.

Settings are not per-save. A value a mod needs to keep with a save belongs in covered state, or under sfall mode in the sidecar.

### 7.5 The mod log

All of this section is `[Safe rule]`.

- `mod_log` appends one line, `<id>: <text>`, or `-: <text>` for a caller with no owner. Text is cut to 200 bytes, and bytes outside 32 to 126 become `?`.
- The log holds the newest 256 lines for the session.
- Each line also goes to the debug log, under the target `vault_debug`, like a diagnostic.
- The F9 overlay shows it on a read-only **Mod log** page, between Camera and Renderer. The page appears only with manifest scripting on. Like the Renderer page, it passes every key but F8 and F9 to the world.
- The word draws nothing in a step's world, is never saved, and is outside every hash. It may be called from any phase a script runs in.

## 8. Out of Scope in Revision 1

| Item | Decision |
| --- | --- |
| Lua or Wasm scripting | Not in this revision. Mods are `.INT` scripts. Either would add large dependencies (Decision D-5) |
| Custom HUD drawing | Not in this revision. Hooks never draw, and no word here draws (Decision D-6) |
| Networked mods | Not in this revision. No word or manifest key reaches a network (Decision D-7) |

## 9. Synthetic Test Fixtures

All data is synthetic, and everything is in `tests/fixtures/spec024a/`. Its `README.md` explains each file.

| File | Coverage |
| --- | --- |
| `vault-mod.toml` | Vector N1: a mod whose script registers itself |
| `vault-mod-row.toml` | Vector N2: the same mod with a `[[hooks]]` row |
| `gl_cheapap.int.hex` | Section 9.1's program, as `OFFSET: bytes` lines |
| `hook-order.tsv` | Section 9.2's order vectors O1 to O11 |
| `hook-points.tsv` | Section 3's list, with every oracle row. Its `fires` column is `yes`, `no`, or `mod` for the mod block alone |
| `mod-words.tsv` | Section 7.3's table, for the stack model |

### 9.1 The program

`gl_cheapap.int` registers one hook and overrides AP cost. It is 250 bytes, with no omitted range.

```text
0000: 80 02 C0 01 00 00 00 12 80 0D C0 01 00 00 00 AA
0010: 80 04 80 10 80 1A 80 20 80 1A 80 21 80 1A 80 22
0020: 80 1A 80 23 80 24 80 25 80 26 00 00 00 03 00 00
0030: 00 06 00 00 00 00 00 00 00 00 00 00 00 00 00 00
0040: 00 BC 00 00 00 00 00 00 00 0E 00 00 00 00 00 00
0050: 00 00 00 00 00 00 00 00 00 BC 00 00 00 00 00 00
0060: 00 16 00 00 00 00 00 00 00 00 00 00 00 00 00 00
0070: 00 D2 00 00 00 00 00 00 00 1A 00 06 6D 61 69 6E
0080: 00 00 00 06 73 74 61 72 74 00 00 08 61 70 5F 63
0090: 6F 73 74 00 FF FF FF FF 00 00 00 0A 00 08 61 70
00A0: 5F 63 6F 73 74 00 FF FF FF FF 80 2C C0 01 00 00
00B0: 00 00 80 03 C0 01 00 00 00 BC 80 04 80 2B C0 01
00C0: 00 00 00 02 C0 01 00 00 00 02 83 04 80 2A 80 29
00D0: 80 1C 80 2B C0 01 00 00 00 00 90 01 00 00 00 06
00E0: C0 01 00 00 00 01 83 01 83 08 C0 01 00 00 00 00
00F0: 80 0D 80 2A 80 0C 80 29 80 1C
```

| Item | Value |
| --- | --- |
| Procedure count N | 3, so I = 46 + 24 x 3 = `0x76` |
| Record 0 at `0x2E` | Name offset 6, `main`; body `0xBC` |
| Record 1 at `0x46` | Name offset 14, `start`; body `0xBC`. Index 0 shares the body, as SPEC-004 section 2.2 says of the corpus |
| Record 2 at `0x5E` | Name offset 22, `ap_cost`; body `0xD2` |
| Identifier payload A | 26, so the terminator is at `0x94` and T is `0x98` |
| Literal payload | 10: `ap_cost` at T + 6, so its terminator is at `0xA6` |
| Initial stream | `0xAA` |

| Offset | Bytes | Meaning |
| --- | --- | --- |
| `0xAA` | `80 2C`, push 0, `80 03`, push `0xBC`, `80 04` | SPEC-004 section 2.2's shape: no globals, root count 0, jump to `start` |
| `0xBC` | `80 2B` | `start`: frame |
| `0xBE` | push 2, push 2, `83 04` | `mod_register_hook(HOOK_CALCAPCOST, 2)`: the hook ID below, procedure index 2 on top |
| `0xCC` | `80 2A 80 29 80 1C` | Cleanup, and return to offset 18 |
| `0xD2` | `80 2B` | `ap_cost`: frame |
| `0xD4` | push 0, push static string offset 6, push 1 | Index 0, key `ap_cost`, default 1 |
| `0xE6` | `83 01` | `mod_get_setting`: pops the default and the key, pushes the value |
| `0xE8` | `83 08` | `mod_set_hook_return`: pops the value and index 0 |
| `0xEA` | push 0, `80 0D 80 2A 80 0C 80 29 80 1C` | The placeholder result, and SPEC-004 section 4.1's result-preserving epilogue |

**Result.** Loading it with vector N1's manifest registers procedure 2 on point 2, in the mod block of `cheap-ap`, and leaves both stacks empty with F at -1. A query with default cost 5 then gives ret0 1, and the action costs 1. With `ap_cost` set to 3, it costs 3.

**Stack model.** SPEC-017 section 8.2 gives no finding over the initial stream and both bodies, with section 7's words in the table.

### 9.2 Order vectors

`hook-order.tsv` runs `HOOK_CALCAPCOST` with native cost 5, args `[101, 0, 0, 5, 0]`, on `hs_calcapcost.int` and two mods: `alpha`, the lower layer, and `beta`, the higher. One row is one member's turn. Sfall global scripts `gl_a` to `gl_d` join in O9 and O11.

| Column | Meaning |
| --- | --- |
| `case` | The vector |
| `sfall` | Sfall mode |
| `order`, `member`, `block` | Turn number, member, and block |
| `outcome` | `ok`, `vm-error`, `suspended`, `skipped` (unloaded by an earlier vector), or `return-unconverted` |
| `arg3_read`, `arg3_after` | arg3 when the member starts, and after it |
| `ret0_after` | ret0 after the member, or `unset` |
| `native`, `result` | The native cost, and on the last row the cost the engine uses |

| Case | Shows |
| --- | --- |
| O1 | Sfall first, then lower, then higher: result 5. The reverse order would give 4 |
| O2 | A member that writes nothing leaves the last write: 8 |
| O3 | No write: the native 5 |
| O4 | Sfall mode off: 6 |
| O5a, O5b | A VM error keeps the write, and unloads the member for the next query: 5, then 4 |
| O6a, O6b | A suspending word ends the member without unloading it: 8, then 10 |
| O7 | A negative final return becomes 0 |
| O8 | A return that cannot convert leaves the earlier write: 10 |
| O9 | The full order of seven members: 52134567 |
| O10 | Rows, then a word registration, inside a mod: 512345 |
| O11 | No manifest member: SPEC-024b's order, 512 |

### 9.3 Depth vector

D1 uses a chain on `HOOK_SETGLOBALVAR` whose one member calls `set_global_var` on the next global, reached from its own call, so section 5.4's exception applies. An ordinary script writes global 0.

| Write | Chain depth | Chain | Value |
| --- | ---: | --- | --- |
| Global 0 to global 7 | 1 to 8 | Runs | Stored |
| Global 8 | 9 | Skipped, with one diagnostic | Still stored (SPEC-024b section 6.18) |

Eight chains run, nine writes store, and global 9 is unchanged.

### 9.4 Word vectors

Run each with N1's mod loaded and `ap_cost` at 1, unless a row says otherwise. Stacks have the top on the right.

| Vector | Stack before | Word | Stack after | Rule |
| --- | --- | --- | --- | --- |
| W1 | empty | `0x8300` | `I:1` | The API version |
| W2 | `"ap_cost", I:9` | `0x8301` | `I:1` | A declared key |
| W3 | `"absent", I:9` | `0x8301` | `I:9` | An undeclared key gives the default, with a diagnostic |
| W4 | `I:7, I:9` | `0x8301` | `I:0` | The key is not a string: failed, 2 pops |
| W5 | `"ap_cost", I:9`, no owner | `0x8301` | `I:9` | No owner gives the default |
| W6 | `"ap_cost", I:3` | `0x8302` | empty | Stored, and W2 now gives `I:3` |
| W7 | `"ap_cost", F:2.5` | `0x8302` | empty | Truncated by class `I`, and W2 now gives `I:2` |
| W8 | `"ap_cost", "x"` | `0x8302` | empty | Cannot convert: unchanged, with a diagnostic |
| W9 | `"hi"` | `0x8303` | empty | The log's newest line is `cheap-ap: hi` |
| W10 | `I:2, I:0` | `0x8304` | empty | Procedure 0 with nothing registered: no effect |
| W11 | `I:50, I:2` | `0x8304` | empty | A reserved hook ID: ignored, with a diagnostic |
| W12 | empty | `0x8305` | `I:-1` | Outside a chain |
| W13 | `I:3` | `0x8306` | `I:0` | Outside a chain |
| W14 | `I:3`, in O1's chain, first member | `0x8306` | `I:5` | arg3 |
| W15 | `I:3, I:4`, in O1's chain | `0x8307` | empty | arg3 is now 4 |
| W16 | `I:0, I:2`, in O1's chain | `0x8308` | empty | ret0 is 2 |
| W17 | `I:1, I:2`, in O1's chain | `0x8308` | empty | The point has one return: ignored |
| W18 | empty, on a local map | `0x8309` | `I:2` | The bit `0x2` |
| W19 | empty stack | `0x8303` | VM error | Underflow stays a VM error |

**Load-time vectors.** Build minimal programs as SPEC-017 section 7.1 does.

| Word | Manifest scripting off | Manifest scripting on | On, with sfall mode |
| --- | --- | --- | --- |
| `0x8300` | Refused at load | Loads | Loads |
| `0x8309` | Refused at load | Loads | Loads |
| `0x830A` | Refused at load | Refused at load | Refused at load |
| `0x83FF` | Refused at load | Refused at load | Refused at load |
| `0x8281` | As SPEC-017 Vector P | As SPEC-017 Vector P | Loads |

### 9.5 Manifest vectors

| Vector | Manifest | Result |
| --- | --- | --- |
| N1 | `vault-mod.toml` | Accepted, scripted. One member, in `cheap-ap`'s block, after `start` runs |
| N2 | `vault-mod-row.toml` | Accepted, scripted. One member: `start`'s `mod_register_hook` finds the row and keeps its position, so N2's result equals N1's |
| N3 | N1 without `id` | Stops the launch |
| N4 | N1 without `api` | Stops the launch |
| N5 | N1 with `api = 2` | Stops the launch |
| N6 | N2 with `point = "HOOK_CALCAPCOSTS"` | Stops the launch |
| N7 | N1 with `file = "hs_tohit.int"` | Stops the launch |
| N8 | N1 naming a file absent from `scripts/` | Stops the launch |
| N9 | N2 with its row repeated | Stops the launch |
| N10 | N1 enabled in two layers | Stops the launch, naming both |
| N11 | N1 in a disabled layer | Accepted, not scripted, no word in the set |
| N12 | N1 without its script rows, keeping `[settings]` | Accepted, not scripted |
| N13 | N2 with `point = "HOOK_ONDEATH"` | Accepted. One diagnostic at setup, and nothing runs |
| N14 | N2 with `procedure = "absent"` | Accepted. One diagnostic at setup, and nothing runs |

The `mod_stack` record of N1 is section 2.3's example: `EE7C38513A262548` is the FNV-1a 64 of `vault-mod.toml`'s 264 bytes.

## 10. Required Tests

1. Every row of `hook-order.tsv`, on the engine's chain: the turn order, the outcome, ret0 after each turn, and the result.
2. Section 9.1's program, through the loader and the chain, on N1 and N2: the registration, ret0 1, and ret0 3 with the setting changed. SPEC-017 section 8.2's model gives no finding.
3. Vector D1.
4. Every W vector, the load-time vectors, and every N vector.
5. With no scripted mod enabled, every SPEC-021 golden, every SPEC-024b vector, and every replay digest is unchanged.
6. With sfall mode on and no scripted mod, SPEC-024b's vectors H1 to H17 are unchanged.
7. A scripted mod on all fifteen points whose members return nothing and draw nothing leaves the `state_hash` and `combat_hash` columns of a checkpoint stream as they are with no mod.
8. Two runs of one log, with a member that draws random numbers, give identical checkpoint streams on two platforms.
9. A test double that fails when a chain starts in phase 3 or phase 4, run over a replay with every point attached.
10. A replay with a changed manifest byte, a changed layer order, or a missing layer is refused before the first step, naming the first `id` that differs.
11. A log recorded with a changed setting replays to the same stream with no settings file present, and writes no file.
12. `HOOK_ADJUSTFID`: each of section 3.3's five paths runs its mod block once, a valid ret0 is stored, an invalid one is ignored, an open inventory's figure is unchanged until the window closes, and a member of its sfall block never runs.
13. Re-entry, membership, and error rules of sections 5.3 and 5.4, including a member that registers another member during a chain.
14. `hook-points.tsv` has every row of `sfall-hooks.tsv`, with the same IDs, names, and types.

## 11. Decisions

Each decision is marked as an owner instruction or a Team A default.

| ID | Decision | Class | Why |
| --- | --- | --- | --- |
| D-1 | One hook interface serves `hs_*.int` scripts and manifest mods, with one list of points | Owner instruction (brief of 2026-10-31) | Two mechanisms would drift |
| D-2 | This spec defines how a manifest attaches scripts and how global scripts load, taking up SPEC-024b Decision D-21 | Owner instruction | SPEC-024b says its rules yield here |
| D-3 | Order, return combination, errors, and the depth limit keep SPEC-021's state hash and checkpoint stream identical across runs and platforms, and hooks never run while drawing | Owner instruction | The brief's requirement |
| D-4 | A word range is reserved, colliding with neither SPEC-017 nor SPEC-024b, with first words for settings and the debug overlay | Owner instruction | The brief's requirement |
| D-5 | Lua or Wasm scripting is out of scope in revision 1 | Owner instruction | Large dependencies |
| D-6 | Custom HUD drawing is out of scope in revision 1 | Owner instruction | The brief's exclusion. It also keeps hooks out of Presentation |
| D-7 | Networked mods are out of scope in revision 1 | Owner instruction | The brief's exclusion. SPEC-021 leaves lockstep to SPEC-025 |
| D-8 | The sfall block runs first, then manifest mods, lowest layer first | Team A default | The last writer wins, so this gives the highest layer the last word, as it wins a file. A session with no manifest keeps SPEC-024b's order. The cost is that `register_hook_proc_spec` no longer runs last in the whole chain |
| D-9 | There is no priority key. Load order is the only control | Team A default | One rule the player can already see and change in the launcher |
| D-10 | The word used to register chooses the block, not the owner of the script | Team A default | An sfall mod packaged with a manifest keeps SPEC-024b's order |
| D-11 | Composition, error handling, and depth reuse SPEC-024b's rules, and one depth counter covers both blocks | Team A default | Two limits could multiply to 64 |
| D-12 | The range is `0x8300..=0x83FF`, the top of what dispatch can reach | Team A default | Words at `0x8400` and up are refused at load. `0x8282..=0x82FF` is enhanced range, and is where a later revision of SPEC-024b's table would grow |
| D-13 | Ten words are defined: the brief's settings and log words, plus the version, registration, three context words, a hook ID, and the game mode | Team A default | With sfall mode off, a mod has no other way to read an argument, write a return, or read the new mode in `HOOK_GAMEMODECHANGE` |
| D-14 | The rest of the range is refused at load. These words take SPEC-024b's failed-operand rule, even with sfall mode off | Team A default | A missing word then fails at the check, not partway through a script. Mod scripts should not stop on a bad operand, for SPEC-024b's reason |
| D-15 | No slot records a mod list, and a mod has no per-save data of its own | Team A default | Saves stay unchanged. A later revision can add a sidecar |
| D-16 | Manifest scripting turns on from the rows themselves, under both profiles, under either preset | Team A default | Enabling the layer is already an opt-in. The hook boundaries anchor on specs both profiles share, except `HOOK_ENCOUNTER` |
| D-17 | `HOOK_ADJUSTFID` fires for its mod block only, with boundary text from SPEC-005 section 3.8, on the player object only. arg1 is the stored art ID. The open inventory figure stays unhooked. The sfall block stays inert until the research side gives it a boundary | Team A default | The brief names art change. SPEC-024b has only its row, and this spec may not use sfall's documents, so the meaning is unchecked against sfall. An Et Tu script uses this hook, so firing it on a guessed meaning could break that mod. A boundary in Presentation would break D-3 |
| D-18 | Manifests add `id` and `api`. `point` uses the public hook names. `hs_` file names are refused, and one script has one owner | Team A default | `name` is free text and cannot key storage. Public names keep one list. The other two rules stop one file running in two roles |
| D-19 | `signal_close_game`'s `HOOK_GAMEMODECHANGE` run is the last thing in phase 2 | Team A default | SPEC-024b section 4.11 says it runs after the Simulation phase, and section 6.6 forbids a chain in phases 3 and 4 |
| D-20 | Settings are declared and typed, kept in `config` as `mod-settings-<id>.toml`, and committed at the end of phase 2. There is no launcher page in this revision | Team A default | A typed table cannot be poisoned by a bad write. `config` already holds per-host choices. Settings are options, not save data |
| D-21 | Replays get `--mod-list` and a `mod_stack` record, compared before the first step | Team A default | A replay had no way to mount layers, and a different stack would diverge silently |
| D-22 | The mod log is a 256-line ring, on a read-only overlay page, outside every hash | Team A default | Bounded, and it cannot affect a run |
| D-23 | A chain's members are fixed at the query, and a member already running is skipped, except SPEC-024b section 6.18's case | Team A default | Both make the outcome independent of when a registration lands. The re-entry rule is what `docs/REFERENCE.md` records |
| D-24 | The limits of a point apply once, to the final value | Team A default | SPEC-024b section 6.5's last-writer rule implies it, and vector O7 pins it |
| D-25 | `[[global_scripts]]` carries `repeat` and `type` | Team A default | `set_global_script_repeat` does not exist with sfall mode off |
| D-26 | A return that cannot convert leaves an earlier member's write standing. Vector O8 pins it | Team A default | SPEC-024b section 6.5 says such a value "leaves that return unset", which could also mean clearing it. Its rule that a return is overridden only by writing that index points to this reading |

## 12. Clean-Room Verification Checklist

- [x] No file path under a source directory, no 40-character hexadecimal string, and no decompiled or disassembled code
- [x] Modules named by Rust path only
- [x] Every normative rule tagged `[Must-match]`, `[May-improve]`, or `[Safe rule]`
- [x] A Provenance line, a revision history, and a Decisions section that marks owner instructions and Team A defaults
- [x] Synthetic vectors only
- [x] No research-vault material, decompilation, re-implementation source, sfall document, or game asset consulted, and no engine Rust code read
- [x] `hook-points.tsv` was checked row by row against `sfall-hooks.tsv`
- [x] Section 9.1's program was computed from SPEC-004's layout and stepped through both procedures by a small interpreter written from SPEC-004. The built `asset-audit --dump-vm-trace` accepted a copy with its three mod words swapped for words it knows, and traced it to `ended`
- [x] `hook-order.tsv` was computed by a small model of section 5 and checked by hand
- [ ] The research linter was not run: Team A has no access to it
