@AGENTS.md

## Clean-Room & Fidelity Policy (Enforced)
1. **Strict Clean-Room Wall:** Never attempt to read, search for, or reference decompilations or re-implementations of Fallout (`fallout1-re`, `fallout2-ce`, `sfall`, etc.). Implement strictly from approved `SPEC-*.md` documents in this repository.
2. **Compatibility Classes:**
   - **Must-match:** Gameplay-visible formulas, script opcodes, file/save binary layouts, LZSS framing, FID encoding, and route costs/caps must match the spec and oracle vectors exactly.
   - **May-improve:** Reference engine memory bugs (uninitialized reads, out-of-bounds array slots, infinite decode loops, truncated path return segments) must NOT be replicated as undefined behavior; return explicit Rust `Result` errors or apply safe bounds.

### Clean-Room Ingestion Rule
- The agent MUST reject any spec containing file paths with `src/`, 40-character git hashes, or raw C decompilation listings.
- Do NOT attempt to redact or sanitize incoming files locally on the builder host. Fail fast and request a sanitized spec from the research vault.

## Multi-Agent Concurrency Rules (Hard Cap: 6 Subagents)
1. **Concurrency Cap:** You may spawn up to **6 concurrent subagents** in a single batched `Task` tool call. Never exceed 6 active subagents at any time.
2. **Disjoint File Ownership:** Every subagent MUST be assigned an explicit, non-overlapping list of files to edit. Two concurrent subagents must NEVER edit the same file.
3. **Zero Target Bloat:** Subagents MUST NOT create `target/agent-*` or `target/verify` directories. Subagents should run `cargo check -p <crate>` or targeted `cargo test -p <crate> --lib <filter>` using the shared `target/` directory, or leave final compilation to the Lead Agent.
4. **Lead Agent Integration & Gates:** Once the subagent batch finishes, the Lead Agent performs any cross-crate glue edits and runs the 4 workspace gates once (`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask wasm-check`).

## Multi-Agent Concurrency Rules (Hard Cap: 6 Subagents)
1. **Concurrency Cap:** You may spawn up to **6 concurrent subagents** in a single batched `Task` tool call. Never exceed 6 active subagents at any time.
2. **Disjoint File Ownership:** Every subagent MUST be assigned an explicit, non-overlapping list of files to edit. Two concurrent subagents must NEVER edit the same file.
3. **Zero Target Bloat:** Subagents MUST NOT create `target/agent-*` or `target/verify` directories. Subagents should run `cargo check -p <crate>` or targeted `cargo test -p <crate> --lib <filter>` using the shared `target/` directory, or leave final compilation to the Lead Agent.
4. **Lead Agent Integration & Gates:** Once the subagent batch finishes, the Lead Agent performs any cross-crate glue edits and runs the 4 workspace gates once (`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask wasm-check`).
