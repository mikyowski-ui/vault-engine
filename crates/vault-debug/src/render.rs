//! SPEC-022 §3.7 and §3.12 the world-canvas renderer's metrics, as the F9
//! overlay shows them: the render path asked for, the instanced path's
//! executor, how each refresh of the last frame was drawn (by the
//! reference path, by the instanced path with its instance, draw-call and
//! snapshot counts, or refused and drawn by the reference path), and the
//! session's refusals by cause.
//!
//! The metrics are plain data filled by the frontend from the renderer's
//! reports; nothing here reads or changes world state, and the render
//! path is never part of a log or a save (SPEC-022 §3.13).
use std::fmt;

/// How the refreshes of one frame were drawn. A frame is one or more
/// refreshes: one for the 640-by-380 map view, one per tile of the
/// `enhanced` world canvas.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameRender {
    /// Refreshes drawn by the reference path because it was asked for.
    pub reference: usize,
    /// Refreshes drawn by the instanced path.
    pub instanced: usize,
    /// Refreshes the instanced path refused, drawn by the reference path.
    pub refused: usize,
    /// The instanced refreshes' sprite instances (after culling).
    pub instances: usize,
    /// Their draw calls: one per batch (§3.7).
    pub draws: usize,
    /// Their snapshot copies: one per read batch (§3.7).
    pub snapshots: usize,
}

impl FrameRender {
    /// A refresh the reference path drew as asked.
    pub fn reference(&mut self) {
        self.reference += 1;
    }
    /// A refresh the instanced path drew: its instances, batches and read
    /// batches.
    pub fn instanced(&mut self, instances: usize, batches: usize, read_batches: usize) {
        self.instanced += 1;
        self.instances += instances;
        self.draws += batches;
        self.snapshots += read_batches;
    }
    /// A refresh the instanced path refused; the reference path drew it.
    pub fn refused(&mut self) {
        self.refused += 1;
    }
    /// Every refresh of the frame.
    pub fn refreshes(&self) -> usize {
        self.reference + self.instanced + self.refused
    }
}

/// The renderer's state for the overlay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderMetrics {
    /// The render path asked for: `reference` or `instanced`.
    pub path: String,
    /// The instanced path's executor once known (a GPU adapter's name, or
    /// the CPU executor); `None` on the reference path or before the
    /// first refresh.
    pub executor: Option<String>,
    /// The last frame drawn.
    pub frame: FrameRender,
    /// Every refresh this session.
    pub session: FrameRender,
    /// Refused refreshes this session, by cause, in cause order.
    pub refusals: Vec<(String, usize)>,
}

impl RenderMetrics {
    /// The overlay's lines: the path and executor, the last frame's
    /// refreshes, the instanced ones' counts, and each refusal cause.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![match &self.executor {
            Some(executor) => format!("render path: {} ({executor})", self.path),
            None => format!("render path: {}", self.path),
        }];
        let frame = &self.frame;
        lines.push(format!(
            "last frame: {} refresh{}, {} instanced, {} fell back",
            frame.refreshes(),
            if frame.refreshes() == 1 { "" } else { "es" },
            frame.instanced,
            frame.refused,
        ));
        lines.push(format!(
            "instances {}  draws {}  snapshots {}",
            frame.instances, frame.draws, frame.snapshots
        ));
        let session = &self.session;
        lines.push(format!(
            "session: {} refreshes, {} instanced, {} fell back",
            session.refreshes(),
            session.instanced,
            session.refused
        ));
        if self.refusals.is_empty() {
            lines.push("fallbacks: none".into());
        } else {
            lines.push("fallbacks this session:".into());
            for (cause, count) in &self.refusals {
                lines.push(format!("  {cause}: {count}"));
            }
        }
        lines
    }
}

impl fmt::Display for RenderMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for line in self.lines() {
            writeln!(f, "{line}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_counts_each_refresh_by_how_it_was_drawn() {
        let mut frame = FrameRender::default();
        frame.instanced(12, 3, 1);
        frame.instanced(8, 2, 0);
        frame.refused();
        frame.reference();
        assert_eq!(
            frame,
            FrameRender {
                reference: 1,
                instanced: 2,
                refused: 1,
                instances: 20,
                draws: 5,
                snapshots: 1,
            }
        );
        assert_eq!(frame.refreshes(), 4);
    }

    #[test]
    fn the_overlay_lines_name_the_path_counts_and_causes() {
        let reference = RenderMetrics {
            path: "reference".into(),
            frame: FrameRender {
                reference: 1,
                ..FrameRender::default()
            },
            session: FrameRender {
                reference: 9,
                ..FrameRender::default()
            },
            ..RenderMetrics::default()
        };
        assert_eq!(
            reference.lines(),
            [
                "render path: reference",
                "last frame: 1 refresh, 0 instanced, 0 fell back",
                "instances 0  draws 0  snapshots 0",
                "session: 9 refreshes, 0 instanced, 0 fell back",
                "fallbacks: none",
            ]
        );
        let mut frame = FrameRender::default();
        frame.instanced(15, 8, 3);
        frame.refused();
        let instanced = RenderMetrics {
            path: "instanced".into(),
            executor: Some("CPU executor".into()),
            frame,
            session: FrameRender {
                instanced: 40,
                refused: 9,
                ..FrameRender::default()
            },
            refusals: vec![
                ("peek mismatch".into(), 2),
                ("unexpressible draw".into(), 7),
            ],
        };
        assert_eq!(
            instanced.to_string(),
            "render path: instanced (CPU executor)\n\
             last frame: 2 refreshes, 1 instanced, 1 fell back\n\
             instances 15  draws 8  snapshots 3\n\
             session: 49 refreshes, 40 instanced, 9 fell back\n\
             fallbacks this session:\n\
             \x20 peek mismatch: 2\n\
             \x20 unexpressible draw: 7\n"
        );
    }
}
