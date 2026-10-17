//! Clean-room asset access. Binary format parsers await approved specifications.

pub mod vfs;

pub use vfs::{LocalDirProvider, Vfs, VfsError, VfsPath};
