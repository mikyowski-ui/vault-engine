//! Dependency-free synthetic fixture builders, shared by the workspace's tests
//! and by `cargo xtask fixtures`. The builders and their data stay under
//! `tests/fixtures/`; this crate compiles them once.
//!
//! `tests/fixtures/items/support.rs` is not part of it: it implements
//! `vault-core` types, so `vault-core` includes that file as its own module.

#[path = "../../../tests/fixtures/archive/dat2.rs"]
pub mod dat2;
#[path = "../../../tests/fixtures/formats/support.rs"]
pub mod formats;
#[path = "../../../tests/fixtures/movie/support.rs"]
pub mod movie;
#[path = "../../../tests/fixtures/sound/support.rs"]
pub mod sound;
#[path = "../../../tests/fixtures/state/support.rs"]
pub mod state;
#[path = "../../../tests/fixtures/vm/support.rs"]
pub mod vm;
#[path = "../../../tests/fixtures/world/support.rs"]
pub mod world;
