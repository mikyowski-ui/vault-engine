//! The vault-debug inspector over the interactive world (`--debug`, or F9
//! during a run): `Inspector::tick` runs once per frame after the world
//! update, watching every GVAR, MVAR and LVAR, and the inspector's live
//! trace records every instruction the world's scripts execute. An overlay
//! on the right of the map view shows the newest variable changes and trace
//! entries; each frame with changes is also logged (target `vault_debug`).
//! The inspector only reads world state; attached, scripts run one
//! observed instruction at a time with unchanged results.
use super::MapPreview;
use crate::interface::Painter;
use std::collections::VecDeque;
use vault_core::ui::ColorKey;
use vault_debug::{Inspector, InspectorConfig, VarScope, WatchList};
use vault_render::IndexedImage;

/// Variable change lines kept for the overlay.
const RECENT: usize = 12;
/// Trace entries shown.
const TRACE_LINES: usize = 16;
const PANEL: [i32; 4] = [352, 0, 288, 380];

pub(crate) struct DebugOverlay {
    inspector: Inspector,
    recent: VecDeque<String>,
}
impl DebugOverlay {
    /// The newest variable change lines.
    pub fn recent_changes(&self) -> impl Iterator<Item = &str> {
        self.recent.iter().map(String::as_str)
    }
    /// The newest trace entries as overlay lines.
    pub fn trace_lines(&self, count: usize) -> Vec<String> {
        self.inspector
            .live_entries(count)
            .iter()
            .map(|entry| {
                let instruction = entry
                    .instruction
                    .as_ref()
                    .map_or_else(|| "<undecodable>".to_owned(), |i| i.text());
                format!("vm{} {:#06x} {}", entry.vm, entry.pc, instruction)
            })
            .collect()
    }
}

impl MapPreview {
    /// Turn the inspector and its live trace on or off.
    pub fn set_debug(&mut self, on: bool) {
        match (on, self.debug.take()) {
            (true, None) => {
                let mut watches = WatchList::new();
                for scope in [VarScope::Global, VarScope::Map, VarScope::Local] {
                    watches.watch_all(scope);
                }
                let mut inspector = Inspector::new(InspectorConfig {
                    watches,
                    track_objects: false,
                    ..InspectorConfig::default()
                });
                inspector.attach(&mut self.host);
                // The first tick sets the baseline.
                inspector.tick(&self.host.world);
                self.debug = Some(DebugOverlay {
                    inspector,
                    recent: VecDeque::new(),
                });
            }
            (false, Some(mut overlay)) => overlay.inspector.detach(&mut self.host),
            (_, kept) => self.debug = kept,
        }
    }
    pub fn debug_enabled(&self) -> bool {
        self.debug.is_some()
    }
    /// One inspector tick after the world update; returns whether anything
    /// changed.
    pub(crate) fn debug_tick(&mut self) -> bool {
        let Some(overlay) = &mut self.debug else {
            return false;
        };
        let frame = overlay.inspector.tick(&self.host.world);
        if frame.is_empty() {
            return false;
        }
        tracing::debug!(target: "vault_debug", "{frame}");
        for change in &frame.vars {
            overlay.recent.push_back(change.to_string());
            if overlay.recent.len() > RECENT {
                overlay.recent.pop_front();
            }
        }
        true
    }
    pub(crate) fn draw_debug(&self, image: &mut IndexedImage) {
        let Some(overlay) = &self.debug else {
            return;
        };
        let painter = Painter {
            assets: &self.host.assets,
            palette: &self.palette,
        };
        image.fill_rect(PANEL, painter.color(ColorKey::BLACK));
        let [x, mut y, width, _] = PANEL;
        let mut line = |image: &mut IndexedImage, text: &str, key: ColorKey| {
            painter.text(image, 101, key, text.as_bytes(), [x + 4, y], width - 8);
            y += 10;
        };
        line(image, "vault-debug (F9 hides)", ColorKey::WHITE);
        line(image, "-- variable changes --", ColorKey::AMBER);
        for change in overlay.recent_changes() {
            line(image, change.trim(), ColorKey::GREEN);
        }
        line(image, "-- script trace --", ColorKey::AMBER);
        for text in overlay.trace_lines(TRACE_LINES) {
            line(image, &text, ColorKey::GREEN);
        }
    }
}
