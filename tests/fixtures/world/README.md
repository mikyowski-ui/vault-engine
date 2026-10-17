# SPEC-003 world fixtures

All data is synthetic. Hex listings use the same explicit-address/repeated-byte
notation as the Phase 1 fixtures.

- `container.pro.hex`: exact section 4.1 fixture A, including the byte at `0x38`
  and unaligned container capacity at `0x39`.
- `empty.map.hex`: exact section 4.2 fixture B; MVARs precede LVARs, omitted tile
  blocks occupy no bytes, and all five script/three elevation counts remain.
- `inventory.hex`: exact section 4.3 fixture C; one root, one quantity-three
  representative child, enclosing elevation and owner restoration.
- `geometry.txt`: all seven section 4.4 hex-center/distance assertions. Tests also
  cover floor/roof picking, stored-column parity, triangular boundaries, border
  stepping, and the explicitly unsafe angular-walk endpoint pair.
- `support.rs`: recipes for all prototype categories/subtypes, mixed-category
  script padding slots, instance trailers, and a complete three-elevation preview
  scene. Test mutations exercise counts, block boundaries, script binding,
  initialization, missing prototype dependencies, and recursion limits.

The generated preview has one floor/roof pair per elevation and five elevation-0
objects spanning items, critters, scenery, and walls. Its art is invented solid
indexed rectangles. Inventory art is not drawn as placed map objects.

```sh
cargo xtask fixtures --output target/world-corpus
cargo run -p asset-audit -- --corpus-dir target/world-corpus --validate-formats
cargo run -p vault-platform -- --corpus-dir target/world-corpus \
  --preview-map maps/synthetic.map
```

The output directory must be empty. The materializer also includes the complete
SPEC-004 program and Phase 1 palettes/fonts/messages.
