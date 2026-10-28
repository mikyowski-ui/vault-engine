//! SPEC-022 §3.1 and §3.12 the world canvas's renderer in the shell: every
//! refresh of the map view and of the `enhanced` world canvas goes through
//! one [`SceneRenderer`] on the session's render path (`render_path`,
//! `--render-path`). The reference path is the default everywhere: the
//! desktop shell, replays and the documentation gallery. The instanced
//! path draws the same canvas, and a refresh it refuses (the `enhanced`
//! shroud, the roof peek, ground-item highlights, a missing device ...)
//! is drawn by the reference path.
//!
//! The setting is a session's, not a world's: one renderer (and with it
//! one GPU device, and one log line per refusal cause) serves every world
//! a session creates. It writes no input-log record and is never saved
//! (§3.13); the F9 overlay's renderer page shows it.
use super::MapPreview;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use vault_debug::{FrameRender, RenderMetrics};
use vault_formats::{MapFile, map::MapObjects};
use vault_render::{
    RenderPath, SceneRenderer,
    map::{MapArt, MapRenderError, MapScene, MapView, RefreshReport},
};

/// The world-canvas renderer one session shares among its worlds, with
/// the session's refresh totals.
#[derive(Clone)]
pub struct SharedRenderer(Arc<Mutex<Shared>>);

pub(crate) struct Shared {
    pub(crate) renderer: SceneRenderer,
    /// Every refresh this session, by how it was drawn.
    pub(crate) session: FrameRender,
}

impl SharedRenderer {
    /// A renderer on `path`; the instanced path's GPU device is created on
    /// its first refresh.
    pub fn new(path: RenderPath) -> Self {
        Self::from_renderer(SceneRenderer::new(path))
    }
    /// A session's renderer from one built elsewhere (the instanced path
    /// on the CPU executor or on a given device, in tests).
    pub fn from_renderer(renderer: SceneRenderer) -> Self {
        Self(Arc::new(Mutex::new(Shared {
            renderer,
            session: FrameRender::default(),
        })))
    }
    pub(crate) fn lock(&self) -> MutexGuard<'_, Shared> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
    /// The path asked for.
    pub fn path(&self) -> RenderPath {
        self.lock().renderer.path()
    }
    /// Every refresh this session, by how it was drawn.
    #[cfg(test)]
    pub fn session_totals(&self) -> FrameRender {
        self.lock().session
    }
    /// How many refreshes the instanced path refused for `cause`.
    #[cfg(test)]
    pub fn refusals(&self, cause: &str) -> usize {
        self.lock().renderer.refusals(cause)
    }
    /// The refusal causes logged so far, each once, by name.
    #[cfg(test)]
    pub fn logged_causes(&self) -> Vec<&'static str> {
        self.lock().renderer.logged_causes().collect()
    }
}

impl Default for SharedRenderer {
    /// The reference path (SPEC-022 Decision D-7).
    fn default() -> Self {
        Self::new(RenderPath::Reference)
    }
}

impl std::fmt::Debug for SharedRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SharedRenderer")
            .field(&self.path().name())
            .finish()
    }
}

/// One frame's refreshes through a locked renderer, counted as they are
/// drawn.
pub(crate) struct FrameRefreshes<'a> {
    shared: MutexGuard<'a, Shared>,
    pub(crate) counts: FrameRender,
}

impl<'a> FrameRefreshes<'a> {
    pub(crate) fn new(shared: &'a SharedRenderer) -> Self {
        Self {
            shared: shared.lock(),
            counts: FrameRender::default(),
        }
    }
    /// One whole refresh of `view` on the session's path.
    pub(crate) fn render<A: MapArt>(
        &mut self,
        map: &MapFile,
        objects: &MapObjects,
        view: MapView,
        art: &A,
        scene: MapScene,
    ) -> Result<vault_render::IndexedImage, MapRenderError> {
        let Shared { renderer, session } = &mut *self.shared;
        let image = renderer.render(map, objects, view, art, scene)?;
        for counts in [&mut self.counts, session] {
            match renderer.last_refresh() {
                Some(RefreshReport::Instanced(drawn)) => {
                    counts.instanced(drawn.instances, drawn.batches, drawn.read_batches)
                }
                Some(RefreshReport::Refused(_)) => counts.refused(),
                Some(RefreshReport::Reference) | None => counts.reference(),
            }
        }
        Ok(image)
    }
}

impl MapPreview {
    /// The session's world-canvas renderer.
    #[cfg(test)]
    pub fn scene_renderer(&self) -> &SharedRenderer {
        &self.scene_renderer
    }
    /// Draw this world's canvas with `renderer` from the next frame on
    /// (the session's, shared by its worlds).
    pub fn set_scene_renderer(&mut self, renderer: SharedRenderer) {
        self.scene_renderer = renderer;
        self.render_frame = FrameRender::default();
        self.thaw_view();
        self.frame = None;
    }
    /// How the last frame's refreshes were drawn.
    #[cfg(test)]
    pub fn last_frame_render(&self) -> FrameRender {
        self.render_frame
    }
    /// The F9 overlay's renderer metrics: the path, the instanced path's
    /// executor, the last frame's counts and the session's refusals.
    pub fn render_metrics(&self) -> RenderMetrics {
        let shared = self.scene_renderer.lock();
        let renderer = &shared.renderer;
        let path = renderer.path();
        let refused = |cause: &str| renderer.refusals(cause) > 0;
        let executor = match (path, renderer.gpu()) {
            (RenderPath::Reference, _) => None,
            (RenderPath::Instanced, Some(gpu)) => Some(format!("GPU {}", gpu.device().adapter())),
            (RenderPath::Instanced, None) if refused("no device") || refused("device lost") => {
                Some("no GPU device: reference".into())
            }
            (RenderPath::Instanced, None) if renderer.last_refresh().is_some() => {
                Some("CPU executor".into())
            }
            (RenderPath::Instanced, None) => None,
        };
        RenderMetrics {
            path: path.name().into(),
            executor,
            frame: self.render_frame,
            session: shared.session,
            // Every cause refused so far (each logged once), by name.
            refusals: renderer
                .logged_causes()
                .map(|cause| (cause.to_owned(), renderer.refusals(cause)))
                .collect(),
        }
    }
}

#[cfg(test)]
#[path = "scene_render_tests.rs"]
mod tests;
