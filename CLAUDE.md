@AGENTS.md

## Clean-Room & Fidelity Policy (Enforced)
1. **Strict Clean-Room Wall:** Never attempt to read, search for, or reference decompilations or re-implementations of Fallout (`fallout1-re`, `fallout2-ce`, `sfall`, etc.). Implement strictly from approved `SPEC-*.md` documents in this repository.
2. **Compatibility Classes:**
   - **Must-match:** Gameplay-visible formulas, script opcodes, file/save binary layouts, LZSS framing, FID encoding, and route costs/caps must match the spec and oracle vectors exactly.
   - **May-improve:** Reference engine memory bugs (uninitialized reads, out-of-bounds array slots, infinite decode loops, truncated path return segments) must NOT be replicated as undefined behavior; return explicit Rust `Result` errors or apply safe bounds.
