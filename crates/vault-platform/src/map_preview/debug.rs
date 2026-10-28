//! The vault-debug inspector over the interactive world (`--debug`, or F9
//! during a run): `Inspector::tick` runs once per frame after the world
//! update, watching every GVAR, MVAR and LVAR, and the inspector's live
//! trace records every instruction the world's scripts execute. An overlay
//! on the right of the map view shows the newest variable changes and trace
//! entries; each frame with changes is also logged (target `vault_debug`).
//! The inspector only reads world state; attached, scripts run one
//! observed instruction at a time with unchanged results.
//!
//! The overlay is also a deterministic editor. F8 cycles its pages: the
//! watch view, the object inspector (a map click selects the object under
//! the pointer instead of acting while the overlay is open), the GVAR and
//! MVAR editors, the map jump and the item spawner, and in the unlocked
//! `enhanced` view a camera page (F: the unbounded free camera with the
//! roofs cut away; C: the camera demo), whose presentation actions are not
//! debug commands and never reach the world. The last page, the renderer,
//! shows the SPEC-022 render path, the last frame's refreshes with their
//! instance, draw-call and snapshot counts, and the session's fallbacks to
//! the reference path ([`MapPreview::render_metrics`]). Editing is keyboard
//! driven: Up/Down (PageUp/PageDown by 10) choose a row, digits and Minus
//! type a value, Backspace deletes, Enter commits, H fully heals the
//! inspected critter. The overlay never changes the world itself: each
//! commit becomes a [`DebugCommand`] in an outbox that the shell drains
//! ([`MapPreview::take_debug_commands`]), logs, and applies
//! ([`MapPreview::apply_debug`]), so a replay of the log applies each
//! command exactly once.
use super::MapPreview;
use crate::{input::key_digit, interface::Painter};
use std::collections::VecDeque;
use vault_core::{
    WorldError, assets::WorldAssets, debug::DebugCommand, ui::ColorKey, worldmap::MAP_COUNT,
};
use vault_debug::{Inspector, InspectorConfig, VarScope, WatchList};
use vault_formats::map::ObjectState;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// Variable change lines kept for the overlay.
const RECENT: usize = 12;
/// Trace entries shown.
const TRACE_LINES: usize = 16;
const PANEL: [i32; 4] = [352, 0, 288, 380];
/// The camera page's "Export Current Setup as Mod" button (E presses it),
/// below the page's six lines.
pub(crate) const EXPORT_BUTTON: [i32; 4] = [PANEL[0] + 4, 62, PANEL[2] - 8, 14];
/// Rows of a variable or map list shown around the cursor.
const LIST_ROWS: usize = 18;
/// The longest typed entry.
const ENTRY_LEN: usize = 11;

/// The overlay's pages, in F8 order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    /// Read-only: variable changes and the script trace.
    Watch,
    Inspect,
    Globals,
    MapVars,
    MapJump,
    Spawn,
    /// The `enhanced` camera's presentation tools (after the spawner, only
    /// in the unlocked enhanced view): the free camera and the camera demo.
    Camera,
    /// Read-only: the world canvas's render path and its metrics
    /// (SPEC-022 §3.7, §3.12), after the spawner and the camera page.
    Renderer,
}
impl Page {
    fn next(self) -> Self {
        match self {
            Self::Watch => Self::Inspect,
            Self::Inspect => Self::Globals,
            Self::Globals => Self::MapVars,
            Self::MapVars => Self::MapJump,
            Self::MapJump => Self::Spawn,
            Self::Spawn | Self::Camera => Self::Renderer,
            Self::Renderer => Self::Watch,
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Watch => "watch",
            Self::Inspect => "inspect",
            Self::Globals => "GVAR editor",
            Self::MapVars => "MVAR editor",
            Self::MapJump => "map jump",
            Self::Spawn => "item spawner",
            Self::Camera => "enhanced camera",
            Self::Renderer => "renderer",
        }
    }
    /// The first row a page's cursor starts on.
    fn first_row(self) -> usize {
        match self {
            Self::MapJump | Self::Spawn => 1,
            _ => 0,
        }
    }
}

pub(crate) struct DebugOverlay {
    inspector: Inspector,
    recent: VecDeque<String>,
    pub(crate) page: Page,
    /// The inspected object's map object id (stable across runs).
    pub(crate) selected: Option<u32>,
    /// The page's row: a variable index, a map number, or the spawn
    /// quantity.
    pub(crate) cursor: usize,
    /// The map jump's elevation.
    elevation: u32,
    /// Typed digits: a value, a tile, or a PID.
    pub(crate) entry: String,
    /// Commands committed and not yet taken by the shell.
    outbox: Vec<DebugCommand>,
    /// The last commit's or application's result.
    pub(crate) note: String,
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
    fn set_page(&mut self, page: Page) {
        self.page = page;
        self.cursor = page.first_row();
        self.entry.clear();
    }
    /// The typed entry as a signed value.
    fn value(&self) -> Option<i32> {
        self.entry.parse().ok()
    }
    fn queue(&mut self, command: DebugCommand) {
        self.note = format!("queued: {command}");
        self.outbox.push(command);
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
                    page: Page::Watch,
                    selected: None,
                    cursor: 0,
                    elevation: 0,
                    entry: String::new(),
                    outbox: Vec::new(),
                    note: String::new(),
                });
            }
            (false, Some(mut overlay)) => overlay.inspector.detach(&mut self.host),
            (_, kept) => self.debug = kept,
        }
    }
    pub fn debug_enabled(&self) -> bool {
        self.debug.is_some()
    }
    /// The commands the overlay committed since the last call, in order.
    /// The desktop shell logs each one (`InputCommand::Debug`) and then
    /// applies it with [`Self::apply_debug`]; a replay discards these and
    /// applies the logged ones instead, so nothing applies twice.
    pub fn take_debug_commands(&mut self) -> Vec<DebugCommand> {
        self.debug
            .as_mut()
            .map(|overlay| std::mem::take(&mut overlay.outbox))
            .unwrap_or_default()
    }
    /// Apply one debugger command to the world through the ordinary engine
    /// paths ([`vault_core::WorldHost::apply_debug`]) and refresh the view.
    /// It works with the overlay closed too (a replayed log). A refused
    /// command changes nothing and is reported on the overlay.
    pub fn apply_debug(&mut self, command: &DebugCommand) -> Result<(), WorldError> {
        self.thaw_view();
        self.pointer.redraw = true;
        let result = self.host.apply_debug(command);
        if let Some(overlay) = &mut self.debug {
            overlay.note = match &result {
                Ok(()) => format!("applied: {command}"),
                Err(error) => format!("refused: {error}"),
            };
        }
        if let Err(error) = &result {
            tracing::warn!(target: "vault_debug", %command, %error, "debug command refused");
        } else {
            tracing::info!(target: "vault_debug", %command, "debug command applied");
        }
        result
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
    /// A left click in the map area while the overlay is open selects the
    /// object under the pointer (or nothing) for the inspector instead of
    /// acting; a click on the panel presses its button, if any
    /// ([`Self::debug_panel_press`]). Right clicks act as usual. Returns
    /// whether the click was taken.
    pub(crate) fn debug_click(&mut self, point: [i32; 2], right: bool) -> bool {
        if right || self.debug.is_none() {
            return false;
        }
        // A point of the `enhanced` world canvas is never on the panel (the
        // enhanced pointer routing takes panel clicks itself).
        let [x, y, width, height] = PANEL;
        let on_panel = !self.pointer.world
            && (x..x + width).contains(&point[0])
            && (y..y + height).contains(&point[1]);
        if on_panel {
            self.debug_panel_press(point);
            return true;
        }
        let picked = self
            .object_at(point)
            .and_then(|object| self.host.world.get(object))
            .map(|object| object.raw.id);
        let Some(overlay) = &mut self.debug else {
            return false;
        };
        overlay.selected = picked;
        overlay.set_page(Page::Inspect);
        overlay.note = match picked {
            Some(id) => format!("selected object {id:#x}"),
            None => "nothing under the pointer".into(),
        };
        self.thaw_view();
        true
    }
    /// A press on the open panel at frame pixel `ui`: the camera page's
    /// export button acts, any other spot does nothing. The panel takes
    /// the press either way.
    pub(crate) fn debug_panel_press(&mut self, ui: [i32; 2]) {
        let [x, y, width, height] = EXPORT_BUTTON;
        let camera = self
            .debug
            .as_ref()
            .is_some_and(|overlay| overlay.page == Page::Camera);
        if camera && (x..x + width).contains(&ui[0]) && (y..y + height).contains(&ui[1]) {
            let note = self.export_note();
            if let Some(overlay) = &mut self.debug {
                overlay.note = note;
            }
            self.thaw_view();
        }
    }
    /// Export the setup; the overlay's note on how it went.
    fn export_note(&mut self) -> String {
        match self.export_setup() {
            Ok(folder) => format!("exported to {}", folder.display()),
            Err(error) => format!("refused: {error}"),
        }
    }
    /// Whether the overlay is open with its panel under frame pixel `ui`.
    pub(crate) fn debug_panel_at(&self, ui: [i32; 2]) -> bool {
        let [x, y, width, height] = PANEL;
        self.debug.is_some() && (x..x + width).contains(&ui[0]) && (y..y + height).contains(&ui[1])
    }
    /// The overlay's keys while it is open: F8 cycles the pages; on every
    /// page but the watch view the editing keys are taken. Returns whether
    /// the key was taken; every other key acts as usual.
    pub(crate) fn debug_key(&mut self, key: KeyCode) -> bool {
        let Some(page) = self.debug.as_ref().map(|overlay| overlay.page) else {
            return false;
        };
        if key == KeyCode::F8 {
            // The camera page follows the spawner in the unlocked
            // `enhanced` view only.
            let next = if page == Page::Spawn && self.enhanced_view().is_some() {
                Page::Camera
            } else {
                page.next()
            };
            if let Some(overlay) = &mut self.debug {
                overlay.set_page(next);
            }
            self.thaw_view();
            return true;
        }
        // The read-only pages pass every other key through.
        if matches!(page, Page::Watch | Page::Renderer) {
            return false;
        }
        // The camera page's presentation actions (not debug commands: the
        // world never sees them).
        if page == Page::Camera {
            let note = match key {
                KeyCode::KeyF => {
                    let on = self.toggle_free_camera();
                    format!("free camera {}", if on { "on" } else { "off" })
                }
                KeyCode::KeyC => {
                    self.start_camera_demo();
                    "camera demo started".into()
                }
                KeyCode::KeyE => self.export_note(),
                _ => return false,
            };
            if let Some(overlay) = &mut self.debug {
                overlay.note = note;
            }
            self.thaw_view();
            return true;
        }
        let rows = self.debug_rows(page);
        let selected = self.debug.as_ref().and_then(|overlay| overlay.selected);
        let Some(overlay) = &mut self.debug else {
            return false;
        };
        let step = |cursor: usize, delta: isize| {
            let low = page.first_row();
            let high = rows.max(low + 1) - 1;
            cursor.saturating_add_signed(delta).clamp(low, high)
        };
        let digit = key_digit(key).map(char::from);
        match key {
            _ if digit.is_some() => {
                if overlay.entry.len() < ENTRY_LEN {
                    overlay.entry.push(digit.expect("a digit"));
                }
            }
            KeyCode::Minus | KeyCode::NumpadSubtract => {
                // Only values are signed; tiles, PIDs and quantities are not.
                if matches!(page, Page::Inspect | Page::Globals | Page::MapVars) {
                    if let Some(rest) = overlay.entry.strip_prefix('-') {
                        overlay.entry = rest.to_string();
                    } else {
                        overlay.entry.insert(0, '-');
                    }
                }
            }
            KeyCode::Backspace => {
                overlay.entry.pop();
            }
            KeyCode::ArrowUp => overlay.cursor = step(overlay.cursor, -1),
            KeyCode::ArrowDown => overlay.cursor = step(overlay.cursor, 1),
            KeyCode::PageUp => overlay.cursor = step(overlay.cursor, -10),
            KeyCode::PageDown => overlay.cursor = step(overlay.cursor, 10),
            KeyCode::ArrowLeft if page == Page::MapJump => {
                overlay.elevation = (overlay.elevation + 2) % 3;
            }
            KeyCode::ArrowRight if page == Page::MapJump => {
                overlay.elevation = (overlay.elevation + 1) % 3;
            }
            KeyCode::KeyH if page == Page::Inspect => match selected {
                Some(object) => overlay.queue(DebugCommand::HealCritter { object }),
                None => overlay.note = "no object selected".into(),
            },
            KeyCode::Enter | KeyCode::NumpadEnter => {
                let index = overlay.cursor;
                let typed = "type a number first";
                let command = match page {
                    Page::Watch | Page::Camera | Page::Renderer => Err(typed),
                    Page::Inspect => match (selected, overlay.value()) {
                        (None, _) => Err("no object selected"),
                        (Some(object), value) => value
                            .map(|value| DebugCommand::SetObjectLocalVar {
                                object,
                                index,
                                value,
                            })
                            .ok_or(typed),
                    },
                    Page::Globals => overlay
                        .value()
                        .map(|value| DebugCommand::SetGlobalVar { index, value })
                        .ok_or(typed),
                    Page::MapVars => overlay
                        .value()
                        .map(|value| DebugCommand::SetMapVar { index, value })
                        .ok_or(typed),
                    Page::MapJump => {
                        let tile = if overlay.entry.is_empty() {
                            Ok(None)
                        } else {
                            overlay.entry.parse::<u32>().map(Some)
                        };
                        tile.map(|tile| DebugCommand::TeleportMap {
                            map_number: index as i32,
                            elevation: overlay.elevation,
                            tile,
                        })
                        .map_err(|_| typed)
                    }
                    Page::Spawn => overlay
                        .entry
                        .parse::<u32>()
                        .map(|pid| DebugCommand::SpawnItem {
                            pid,
                            quantity: index as u32,
                        })
                        .map_err(|_| typed),
                };
                match command {
                    Ok(command) => {
                        overlay.queue(command);
                        overlay.entry.clear();
                    }
                    Err(problem) => overlay.note = problem.into(),
                }
            }
            _ => return false,
        }
        self.thaw_view();
        true
    }
    /// How many rows a page's cursor moves over.
    fn debug_rows(&self, page: Page) -> usize {
        let world = &self.host.world;
        match page {
            Page::Watch | Page::Camera | Page::Renderer => 0,
            Page::Inspect => self
                .debug
                .as_ref()
                .and_then(|overlay| overlay.selected)
                .and_then(|id| world.reference_for_id(id))
                .and_then(|object| world.get(object))
                .and_then(|object| u32::try_from(object.raw.script_id).ok())
                .and_then(|sid| world.scripts.get(&sid))
                .map_or(0, |script| script.local_count),
            Page::Globals => world.gvars.as_ref().map_or(0, Vec::len),
            Page::MapVars => world.mvars.len(),
            Page::MapJump => MAP_COUNT,
            Page::Spawn => vault_core::debug::MAX_SPAWN as usize + 1,
        }
    }
    /// The inspected object's lines and its LVARs.
    fn inspect_lines(&self, overlay: &DebugOverlay) -> (Vec<String>, Vec<(usize, i32)>) {
        let world = &self.host.world;
        let Some(id) = overlay.selected else {
            return (vec!["click an object on the map".into()], Vec::new());
        };
        let Some(object) = world
            .reference_for_id(id)
            .and_then(|reference| world.get(reference).map(|o| (reference, o)))
        else {
            return (vec![format!("object {id:#x} is gone")], Vec::new());
        };
        let (reference, object) = object;
        let mut lines = vec![
            format!(
                "id {id:#x} {}",
                String::from_utf8_lossy(&object.name).trim()
            ),
            format!(
                "PID {:#010x}  FID {:#010x}",
                object.raw.pid, object.raw.art_id
            ),
            format!(
                "hex {}  elevation {}  flags {:#010x}",
                object
                    .raw
                    .hex
                    .map_or_else(|| "-".into(), |hex| hex.to_string()),
                object.raw.elevation,
                object.raw.flags.0
            ),
        ];
        let sid = u32::try_from(object.raw.script_id).ok();
        let script = sid.and_then(|sid| world.scripts.get(&sid));
        match script {
            Some(script) => {
                let name = self
                    .host
                    .assets
                    .program(script.file)
                    .map_or_else(|_| format!("script file {}", script.file), |(path, _)| path);
                lines.push(format!("script {:#010x} {name}", script.id));
            }
            None => lines.push("no script".into()),
        }
        if let ObjectState::Critter(state) = &object.raw.state {
            lines.push(format!(
                "AI packet {}  team {}  results {:#x}",
                state.ai, state.team, state.results.0
            ));
            lines.push(format!(
                "HP {}/{}  AP {}",
                world.hit_points(reference).unwrap_or(state.hp),
                world.max_hit_points(reference),
                state.ap
            ));
        }
        let locals = script
            .map(|script| {
                (0..script.local_count)
                    .map(|index| {
                        let value = usize::try_from(script.local_base)
                            .ok()
                            .and_then(|base| world.lvars.get(base + index))
                            .copied()
                            .unwrap_or(0);
                        (index, value)
                    })
                    .collect()
            })
            .unwrap_or_default();
        (lines, locals)
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
        line(
            image,
            &format!("vault-debug: {} (F9 hides, F8 page)", overlay.page.title()),
            ColorKey::WHITE,
        );
        // A window of rows around the cursor.
        let window = |count: usize| {
            let start = overlay
                .cursor
                .saturating_sub(LIST_ROWS / 2)
                .min(count.saturating_sub(LIST_ROWS));
            start..count.min(start + LIST_ROWS)
        };
        let marker = |row: usize| if row == overlay.cursor { ">" } else { " " };
        let entry = format!("{}_", overlay.entry);
        match overlay.page {
            Page::Watch => {
                line(image, "-- variable changes --", ColorKey::AMBER);
                for change in overlay.recent_changes() {
                    line(image, change.trim(), ColorKey::GREEN);
                }
                line(image, "-- script trace --", ColorKey::AMBER);
                for text in overlay.trace_lines(TRACE_LINES) {
                    line(image, &text, ColorKey::GREEN);
                }
            }
            Page::Inspect => {
                let (lines, locals) = self.inspect_lines(overlay);
                for text in &lines {
                    line(image, text, ColorKey::GREEN);
                }
                if overlay.selected.is_some() {
                    line(
                        image,
                        "-- LVARs (Enter sets, H full heal) --",
                        ColorKey::AMBER,
                    );
                    for index in window(locals.len()) {
                        let (_, value) = locals[index];
                        line(
                            image,
                            &format!("{} LVAR {index} = {value}", marker(index)),
                            ColorKey::GREEN,
                        );
                    }
                    line(image, &format!("value: {entry}"), ColorKey::WHITE);
                }
            }
            Page::Globals | Page::MapVars => {
                let (label, values): (&str, &[i32]) = if overlay.page == Page::Globals {
                    ("GVAR", self.host.world.gvars.as_deref().unwrap_or(&[]))
                } else {
                    ("MVAR", &self.host.world.mvars)
                };
                line(image, "Up/Down choose, type, Enter sets", ColorKey::AMBER);
                if values.is_empty() {
                    line(image, &format!("no {label}s"), ColorKey::GREEN);
                }
                for index in window(values.len()) {
                    line(
                        image,
                        &format!("{} {label} {index} = {}", marker(index), values[index]),
                        ColorKey::GREEN,
                    );
                }
                line(image, &format!("value: {entry}"), ColorKey::WHITE);
            }
            Page::MapJump => {
                line(image, "Up/Down map, Left/Right elevation", ColorKey::AMBER);
                let tables = self.host.world.rules.tables();
                for number in window(MAP_COUNT).filter(|number| *number >= 1) {
                    line(
                        image,
                        &format!(
                            "{} {number:2} {}",
                            marker(number),
                            tables.map_name(number as i32).unwrap_or("?")
                        ),
                        ColorKey::GREEN,
                    );
                }
                line(
                    image,
                    &format!("elevation {}", overlay.elevation),
                    ColorKey::WHITE,
                );
                line(
                    image,
                    &format!("tile: {entry} (empty: entrance)"),
                    ColorKey::WHITE,
                );
            }
            Page::Camera => {
                line(
                    image,
                    "F free camera, C camera demo, E export",
                    ColorKey::AMBER,
                );
                let lines = self.camera_lines();
                for text in &lines {
                    line(image, text, ColorKey::GREEN);
                }
                // The button over the next lines; the note goes below it.
                let [bx, by, bw, bh] = EXPORT_BUTTON;
                image.fill_rect(EXPORT_BUTTON, painter.color(ColorKey::AMBER));
                image.fill_rect(
                    [bx + 1, by + 1, bw - 2, bh - 2],
                    painter.color(ColorKey::BLACK),
                );
                painter.text(
                    image,
                    101,
                    ColorKey::WHITE,
                    b"Export Current Setup as Mod (E)",
                    [bx + 4, by + 3],
                    bw - 8,
                );
                for _ in lines.len()..6 {
                    line(image, "", ColorKey::WHITE);
                }
            }
            Page::Renderer => {
                line(image, "-- world canvas (SPEC-022) --", ColorKey::AMBER);
                for text in self.render_metrics().lines() {
                    line(image, &text, ColorKey::GREEN);
                }
            }
            Page::Spawn => {
                line(image, "type the PID (decimal), Enter", ColorKey::AMBER);
                let pid = overlay.entry.parse::<u32>().ok();
                line(
                    image,
                    &format!(
                        "PID: {entry} {}",
                        pid.map_or_else(String::new, |pid| format!("({pid:#010x})"))
                    ),
                    ColorKey::WHITE,
                );
                line(
                    image,
                    &format!("quantity: {} (Up/Down)", overlay.cursor),
                    ColorKey::WHITE,
                );
            }
        }
        if !overlay.note.is_empty() {
            let key = if overlay.note.starts_with("refused") {
                ColorKey::RED
            } else {
                ColorKey::AMBER
            };
            line(image, &overlay.note, key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map_preview::{state_fixture, test_support::interactive};

    fn world(dir: &std::path::Path) -> MapPreview {
        state_fixture::write_corpus(dir);
        let mut preview = interactive(dir);
        preview.tick(16).unwrap();
        preview.image().unwrap();
        preview
    }
    /// The fixture NPC's frame pixel (the replay tests click it there).
    const NPC_PIXEL: [i32; 2] = [336, 170];
    fn npc_id(preview: &MapPreview) -> u32 {
        preview
            .host
            .world
            .iter()
            .find(|(_, object)| object.raw.pid == 0x0100_0001)
            .map(|(_, object)| object.raw.id)
            .unwrap()
    }
    fn press(preview: &mut MapPreview, keys: &[KeyCode]) {
        for key in keys {
            preview.step(*key);
        }
    }
    fn page(preview: &MapPreview) -> Page {
        preview.debug.as_ref().unwrap().page
    }

    #[test]
    fn the_title_shows_internal_hints_only_in_debug_mode() {
        let dir = tempfile::tempdir().unwrap();
        let mut preview = world(dir.path());
        assert_eq!(preview.title(), "Fallout — Vault Engine");
        preview.set_debug(true);
        let title = preview.title();
        assert!(
            title.contains("maps/synthetic.map") && title.contains("elevation 0"),
            "{title}"
        );
        assert!(title.contains("cursor"), "{title}");
    }

    #[test]
    fn with_the_overlay_closed_nothing_is_taken_or_queued() {
        let dir = tempfile::tempdir().unwrap();
        let mut preview = world(dir.path());
        assert!(!preview.debug_key(KeyCode::F8));
        assert!(!preview.debug_key(KeyCode::Enter));
        assert!(!preview.debug_click(NPC_PIXEL, false));
        // The right-click arrow then a left click talks, as in a game.
        assert!(preview.click(NPC_PIXEL, true));
        preview.click(NPC_PIXEL, false);
        preview.release(Some(NPC_PIXEL), false);
        assert!(preview.selected.is_some());
        assert!(preview.take_debug_commands().is_empty());
    }

    #[test]
    fn a_map_click_inspects_and_edits_queue_commands_without_applying_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut preview = world(dir.path());
        press(&mut preview, &[KeyCode::F9]);
        assert_eq!(page(&preview), Page::Watch);
        // The watch view takes no editing key.
        assert!(!preview.debug_key(KeyCode::Digit1));
        // A left click selects the NPC instead of acting.
        assert!(preview.click(NPC_PIXEL, false));
        preview.release(Some(NPC_PIXEL), false);
        let id = npc_id(&preview);
        let overlay = preview.debug.as_ref().unwrap();
        assert_eq!((overlay.page, overlay.selected), (Page::Inspect, Some(id)));
        assert!(preview.selected.is_none(), "the click did not act");
        assert!(!preview.host.world.dialogue.open);
        let lines = preview.inspect_lines(preview.debug.as_ref().unwrap()).0;
        assert!(
            lines.iter().any(|line| line.contains("PID 0x01000001")),
            "{lines:?}"
        );
        assert!(lines.iter().any(|line| line.contains("AI packet")));
        assert!(lines.iter().any(|line| line.starts_with("HP ")));
        assert!(lines.iter().any(|line| line.contains("script 0x04000001")));
        // LVAR 1 = -25, then a full heal.
        press(
            &mut preview,
            &[
                KeyCode::ArrowDown,
                KeyCode::Digit2,
                KeyCode::Digit5,
                KeyCode::Minus,
                KeyCode::Enter,
                KeyCode::KeyH,
            ],
        );
        let sid = preview
            .host
            .world
            .get(preview.host.world.reference_for_id(id).unwrap())
            .unwrap()
            .raw
            .script_id as u32;
        let before = preview.host.world.local(Some(sid), 1);
        let commands = preview.take_debug_commands();
        assert_eq!(
            commands,
            [
                DebugCommand::SetObjectLocalVar {
                    object: id,
                    index: 1,
                    value: -25
                },
                DebugCommand::HealCritter { object: id },
            ]
        );
        assert!(preview.take_debug_commands().is_empty(), "taken once");
        assert_eq!(
            preview.host.world.local(Some(sid), 1),
            before,
            "not applied"
        );
        for command in &commands {
            preview.apply_debug(command).unwrap();
        }
        assert_eq!(preview.host.world.local(Some(sid), 1), -25);
        assert!(preview.debug.as_ref().unwrap().note.starts_with("applied"));
        // A click on empty map clears the selection; the panel takes clicks.
        assert!(preview.click([10, 10], false));
        assert_eq!(preview.debug.as_ref().unwrap().selected, None);
        assert!(preview.click([500, 100], false));
        // Right clicks act as usual (they cycle the cursor).
        assert!(preview.click(NPC_PIXEL, true));
        assert!(preview.take_debug_commands().is_empty());
    }

    #[test]
    fn the_variable_map_and_spawn_pages_emit_their_commands() {
        let dir = tempfile::tempdir().unwrap();
        let mut preview = world(dir.path());
        preview.set_debug(true);
        // GVAR 2 = -12.
        press(&mut preview, &[KeyCode::F8, KeyCode::F8]);
        assert_eq!(page(&preview), Page::Globals);
        press(
            &mut preview,
            &[
                KeyCode::ArrowDown,
                KeyCode::ArrowDown,
                KeyCode::Digit1,
                KeyCode::Digit2,
                KeyCode::NumpadSubtract,
                KeyCode::Enter,
            ],
        );
        // Enter with nothing typed queues nothing.
        press(&mut preview, &[KeyCode::Enter]);
        assert_eq!(
            preview.take_debug_commands(),
            [DebugCommand::SetGlobalVar {
                index: 2,
                value: -12
            }]
        );
        // MVAR rows stop at the last variable.
        press(&mut preview, &[KeyCode::F8]);
        assert_eq!(page(&preview), Page::MapVars);
        let mvars = preview.host.world.mvars.len();
        for _ in 0..mvars + 3 {
            press(&mut preview, &[KeyCode::ArrowDown]);
        }
        assert_eq!(
            preview.debug.as_ref().unwrap().cursor,
            mvars.saturating_sub(1)
        );
        // Map 2, elevation 1, hex 123; then map 65 at its entrance.
        press(&mut preview, &[KeyCode::F8]);
        assert_eq!(page(&preview), Page::MapJump);
        let mut keys = vec![
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowRight,
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Enter,
        ];
        keys.extend([KeyCode::PageDown; 7]);
        keys.extend([KeyCode::ArrowLeft, KeyCode::Enter]);
        press(&mut preview, &keys);
        assert_eq!(
            preview.take_debug_commands(),
            [
                DebugCommand::TeleportMap {
                    map_number: 2,
                    elevation: 1,
                    tile: Some(123)
                },
                DebugCommand::TeleportMap {
                    map_number: 65,
                    elevation: 0,
                    tile: None
                },
            ]
        );
        // Two of PID 1.
        press(&mut preview, &[KeyCode::F8]);
        assert_eq!(page(&preview), Page::Spawn);
        press(
            &mut preview,
            &[
                KeyCode::ArrowDown,
                KeyCode::Digit1,
                KeyCode::Backspace,
                KeyCode::Digit1,
                KeyCode::Enter,
            ],
        );
        let spawn = preview.take_debug_commands();
        assert_eq!(
            spawn,
            [DebugCommand::SpawnItem {
                pid: 1,
                quantity: 2
            }]
        );
        let player = preview.host.world.player;
        let before = preview.host.world.inventory_count(player, 1);
        preview.apply_debug(&spawn[0]).unwrap();
        assert_eq!(preview.host.world.inventory_count(player, 1), before + 2);
        // A refused command is reported and changes nothing.
        assert!(
            preview
                .apply_debug(&DebugCommand::SetGlobalVar {
                    index: 1 << 30,
                    value: 1
                })
                .is_err()
        );
        assert!(preview.debug.as_ref().unwrap().note.starts_with("refused"));
        // F8 reaches the read-only renderer page, which passes keys
        // through and shows the reference path (the default) ...
        press(&mut preview, &[KeyCode::F8]);
        assert_eq!(page(&preview), Page::Renderer);
        assert!(!preview.debug_key(KeyCode::Enter));
        assert!(!preview.debug_key(KeyCode::Digit1));
        assert!(preview.take_debug_commands().is_empty());
        let metrics = preview.render_metrics();
        assert_eq!(metrics.path, "reference");
        assert_eq!(metrics.executor, None);
        assert!(metrics.refusals.is_empty());
        preview.image().unwrap();
        assert_eq!(
            preview.last_frame_render(),
            vault_debug::FrameRender {
                reference: 1,
                ..Default::default()
            }
        );
        // ... and wraps to the watch view, which passes keys through again.
        press(&mut preview, &[KeyCode::F8]);
        assert_eq!(page(&preview), Page::Watch);
        assert!(!preview.debug_key(KeyCode::Enter));
    }

    /// The renderer page shows the metrics' lines: the reference path, then
    /// the instanced path with its executor, the last frame's counts and
    /// the session's; the map view left of the panel is the same on both.
    #[test]
    fn the_renderer_page_shows_the_render_path_and_its_counts() {
        let dir = tempfile::tempdir().unwrap();
        let mut preview = world(dir.path());
        preview.set_debug(true);
        while page(&preview) != Page::Renderer {
            press(&mut preview, &[KeyCode::F8]);
        }
        let reference = preview.image().unwrap();
        let lines = preview.render_metrics().lines();
        assert_eq!(lines[0], "render path: reference");
        assert_eq!(lines[1], "last frame: 1 refresh, 0 instanced, 0 fell back");
        preview.set_scene_renderer(crate::map_preview::test_support::cpu_renderer());
        let instanced = preview.image().unwrap();
        let metrics = preview.render_metrics();
        assert_eq!(metrics.path, "instanced");
        assert_eq!(metrics.executor.as_deref(), Some("CPU executor"));
        let frame = metrics.frame;
        assert_eq!((frame.refreshes(), frame.instanced), (1, 1), "{frame:?}");
        assert!(frame.instances > 0 && frame.draws > 0, "{frame:?}");
        assert_eq!(metrics.session, frame);
        let lines = metrics.lines();
        assert_eq!(lines[0], "render path: instanced (CPU executor)");
        assert_eq!(lines[1], "last frame: 1 refresh, 1 instanced, 0 fell back");
        assert_eq!(
            lines[2],
            format!(
                "instances {}  draws {}  snapshots {}",
                frame.instances, frame.draws, frame.snapshots
            )
        );
        assert_eq!(lines.last().map(String::as_str), Some("fallbacks: none"));
        let view = |image: &IndexedImage| -> Vec<u8> {
            (0..380)
                .flat_map(|y| {
                    let row = (y * image.width()) as usize;
                    image.pixels()[row..row + PANEL[0] as usize].to_vec()
                })
                .collect()
        };
        assert!(view(&reference) == view(&instanced), "the map views differ");
    }
}
