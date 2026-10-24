//! Builders the platform tests share: the synthetic corpus, a world loaded
//! over it, and the local corpus.
use super::{MapPreview, state_fixture};
use std::{path::Path, sync::Arc};
use vault_core::ObjectRef;
use vault_formats::{LocalDirProvider, Vfs, pro::Category};

/// The synthetic corpus in a new temporary directory.
pub(crate) fn synthetic_corpus() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    state_fixture::write_corpus(dir.path());
    dir
}

/// `map` over the corpus at `root`, with the synthetic player sheet.
pub(crate) fn load(root: &Path, map: &str) -> MapPreview {
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(root).unwrap());
    MapPreview::load(vfs, map, "english", Some("premade/synthetic.gcd"), 1).unwrap()
}

/// The synthetic map over the corpus at `root`, with the synthetic player.
pub(crate) fn interactive(root: &Path) -> MapPreview {
    load(root, "maps/synthetic.map")
}

/// The first critter in the world that is not the player.
pub(crate) fn npc(preview: &MapPreview) -> ObjectRef {
    let player = preview.host.world.player;
    preview
        .host
        .world
        .iter()
        .find(|(reference, object)| object.category() == Category::Critter && *reference != player)
        .unwrap()
        .0
}

/// The unpacked corpus: `$VAULT_CORPUS`, else `local-corpus` beside the
/// repository; `None` skips the test.
pub(crate) fn local_corpus() -> Option<Arc<dyn Vfs>> {
    let dir = std::env::var_os("VAULT_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../local-corpus"))
        .join("fallout1-unpacked");
    if !dir.is_dir() {
        eprintln!("skipping: no local corpus at {}", dir.display());
        return None;
    }
    Some(Arc::new(LocalDirProvider::mount(dir).unwrap()))
}
