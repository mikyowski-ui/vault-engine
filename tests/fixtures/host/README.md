# SPEC-005 extension fixtures

- `setters.hex` and `setters-stack.hex`: exact section 4.1 input and output bytes,
  with GVAR index four, self reference `0x10000`, elevation one, and clock 12340.
- `sound.hex`: exact section 4.2 fragment, with two occupied sound slots. Tests
  verify integer-tagged handle `0xA0000002`, statement-only controls, slot release,
  and the float-mode mutation's operand consumption and ended/error flags.
- `dialogue.hex`: exact section 4.3 fragment. Tests supply literal offsets 6 and 14,
  verify begin/animate/end registration, all five `start_gdialog` arguments, reply
  and option construction, and empty final stacks. Both the string-procedure
  mutation and omitted end-registration parameter are exercised.

The fixtures use deterministic in-memory hosts: no actual GUI, sound device,
movie, or original game object is created. Catalog coverage tests visit all 266
slots, including both unregistered holes, and distinguish unsupported/unsafe
operations from accidentally unwired opcodes.
