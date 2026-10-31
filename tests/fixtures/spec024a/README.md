# SPEC-024a engine hook and mod script fixtures

Every file here is synthetic. The Spec-Author sub-agent (Team A) wrote or
computed each one from SPEC-024a alone. No manifest, script or table holds game
data. See SPEC-024a section 9. These vectors live here, not in `oracle/`, which
is reserved for research-side vectors.

Text formats: TSV files have one header row. A `-` means "not used".

- `vault-mod.toml` (264 bytes): vector N1. A tiny manifest whose global script
  registers its own hook.
- `vault-mod-row.toml`: vector N2. The same mod, with a `[[hooks]]` row that
  attaches the same procedure. Its result equals N1's.
- `gl_cheapap.int.hex` (250 bytes, as `OFFSET: bytes` lines of 16 bytes, the form
  of `tests/fixtures/vm/program.int.hex`): a complete SPEC-004 program. `start`
  registers procedure `ap_cost` on `HOOK_CALCAPCOST`, and `ap_cost` sets ret0 to
  the `ap_cost` setting. SPEC-024a section 9.1 walks it line by line.
- `hook-order.tsv`: vectors O1 to O11 of section 9.2, one row per member's turn,
  for two mods and one sfall script on one hook point.
- `hook-points.tsv`: the hook list of section 3 with every row of
  `oracle/SPEC-024/sfall-hooks.tsv`: whether the point fires (`yes`, `no`, or
  `mod` for the mod block alone), under which profiles, its types, the native
  result it overrides, and where its boundary text is.
- `mod-words.tsv`: the words of section 7.3, with the `pops` and `push` columns
  the stack model reads and the operand classes of SPEC-024b section 4.1.
