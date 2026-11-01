//! Builders the platform tests share: the synthetic corpus, a world loaded
//! over it, and the local corpus.
use super::{MapPreview, state_fixture};
use std::{path::Path, sync::Arc};
use vault_core::ObjectRef;
use vault_formats::{LocalDirProvider, Vfs, pro::Category};

/// One row per step of a scripted session, compared with a pinned table in
/// `tests/fixtures/characterization/`. These pin exact behaviour across a
/// refactor: `state_hash` leaves out the clock, the queue and the world map,
/// so each row carries them too. Set `VAULT_BLESS_CHARACTERIZATION=1` to
/// rewrite a table, and only when a change in behaviour is intended.
pub(crate) struct Recorder {
    name: &'static str,
    rows: String,
}

impl Recorder {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            rows: "step\tstate\tcombat\tticks\trng\tcritter\tscreen\tmode\treplay\trequest\tmap\tf1\tf2\tstatus\n"
                .to_owned(),
        }
    }

    /// The row for `preview` as `step` left it.
    pub(crate) fn row(&mut self, step: &str, preview: &MapPreview) {
        let world = &preview.host.world;
        let screen = match &preview.screen {
            super::Screen::Map => "map",
            super::Screen::World(_) => "world",
            super::Screen::Town(_) => "town",
            super::Screen::World2(_) => "world2",
            super::Screen::Town2(_) => "town2",
            _ => "other",
        };
        let f2 = world
            .fallout2
            .as_deref()
            .map(|state| format!("{:?} {:?}", state.position, state.travel));
        self.rows.push_str(&format!(
            "{step}\t{:016x}\t{:016x}\t{}\t{:?}\t{:?}\t{screen}\t{:?}\t{:?}\t{:?}\t{}\t{:?} {:?}\t{f2:?}\t{}\n",
            world.state_hash(),
            world.combat_hash(),
            world.clock.ticks,
            world.rng.state(),
            world.critter_state(world.player),
            preview.hud.mode,
            preview.hud.replay.is_some(),
            world.pending_combat.is_some(),
            world.map.header.map_number,
            world.world_map.saved.position,
            world.world_map.travel,
            String::from_utf8_lossy(&preview.status),
        ));
    }

    /// Compare with the pinned table.
    pub(crate) fn check(self) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/characterization")
            .join(format!("{}.tsv", self.name));
        if std::env::var_os("VAULT_BLESS_CHARACTERIZATION").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &self.rows).unwrap();
            return;
        }
        let pinned = std::fs::read_to_string(&path).unwrap_or_default();
        for (at, (old, new)) in pinned.lines().zip(self.rows.lines()).enumerate() {
            assert_eq!(new, old, "{} line {}", self.name, at + 1);
        }
        assert_eq!(
            self.rows.lines().count(),
            pinned.lines().count(),
            "{} row count",
            self.name
        );
    }
}

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

/// The lock every SPEC-022 GPU test of this crate holds while it creates,
/// uses and drops a device: some drivers (the software Vulkan one among
/// them) crash when test threads create and destroy devices at once, as
/// `vault_render`'s own GPU tests found.
pub(crate) fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The instanced path on a GPU device, for a GPU test (SPEC-022 §4), by the
/// rules of [`vault_render::layers::gpu::test_device`]. The caller holds
/// [`gpu_lock`] until the renderer is dropped.
pub(crate) fn gpu_renderer() -> Option<vault_render::SceneRenderer> {
    vault_render::layers::gpu::test_device().map(vault_render::SceneRenderer::with_device)
}

/// The instanced path on the CPU executor: the same canvases as the GPU
/// executor, on any host.
pub(crate) fn cpu_renderer() -> super::SharedRenderer {
    super::SharedRenderer::from_renderer(vault_render::SceneRenderer::cpu())
}
