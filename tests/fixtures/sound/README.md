# SPEC-010 sound fixtures

Synthetic ACM streams A-H from SPEC-010 sections 4.1-4.2, as offset-addressed
hex. Each is a complete file: the 14-byte little-endian header, then the
LSB-first block bitstream. The expected samples and errors are asserted in
`crates/vault-formats/src/acm_tests.rs`. They were authored from the
specification and contain no game audio.
