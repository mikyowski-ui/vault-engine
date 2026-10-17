# SPEC-004 VM fixtures

- `program.int.hex`: every byte of section 4.1's 282-byte executable image.
- `fragments.txt`: all eleven isolated fragments from section 4.2, with exact
  expected payload-before-tag value-stack bytes.
- `errors.txt`: all section 4.3 instruction-error cases. Slot 342 is rejected by
  the safe dispatcher instead of indexing beyond its table.
- `support.rs`: the complete-image decoder and synthetic host-call/conditional
  stream recipes. These condition streams intentionally export a variable to
  verify that evaluation has no invented purity restriction.

Tests execute the complete image until ended across repeated instruction budgets,
both branch outcomes, the argument-metadata mutation, and unknown flag `0x20`.
They exercise the out-of-file body and literal-length mutations, 341/342-value
stack boundaries, context-return variants, external string copying, scheduling,
registry ownership, host program control, and undefined imported-call rejection.
Additional tests cover typed identifier operands, deterministic mutation smoke
tests, and arithmetic/truthiness quirks outside the minimum vectors.

The fixture is compiled game-style bytecode written specifically for these tests;
it contains no original script, game logic, or decompiled source.
