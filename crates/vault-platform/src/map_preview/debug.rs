//! The vault-debug inspector over the interactive world (`--debug`, or F9
//! during a run): `Inspector::tick` runs once per frame after the world
//! update, watching every GVAR, MVAR and LVAR, and the inspector's live
//! trace records every instruction the world's scripts execute. An overlay
//! on the right of the map view shows the newest variable changes and trace
//! entries; each frame with changes is also logged (target `vault_debug`).
//! The inspector only reads world state; attached, scripts run one
//! observed instruction at a time with unchanged results.
//!
//! The overlay is also a deterministic editor. A tab strip across the top
//! ([`super::debug_ui`]) opens Trace & Stepper, the Hex & Object Inspector,
//! Timers & Explosives, World Spawner & Warp and the Autoplay Bot, and its
//! first tab holds the pages F8 cycles: the
//! watch view, the object inspector (a map click selects the object and the
//! hex under the pointer instead of acting while the overlay is open), the
//! GVAR and MVAR editors, the map jump and the item spawner, and in the
//! unlocked
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
use super::{
    MapPreview,
    debug_ui::{Column, TOP, inside, rows_around},
};
use crate::{input::key_digit, interface::Painter};
use std::collections::VecDeque;
use vault_core::{
    WorldError, assets::WorldAssets, debug::DebugCommand, ui::ColorKey, worldmap::MAP_COUNT,
};
use vault_debug::{Inspector, InspectorConfig, VarScope, WatchList};
use vault_formats::{map::ObjectState, pro::LockFlags};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// Variable change lines kept for the overlay.
const RECENT: usize = 12;
/// Trace entries shown.
const TRACE_LINES: usize = 16;
pub(super) const PANEL: [i32; 4] = [352, 0, 288, 380];
/// The camera page's "Export Current Setup as Mod" button (E presses it),
/// below the page's six lines.
pub(crate) const EXPORT_BUTTON: [i32; 4] = [PANEL[0] + 4, TOP + 52, PANEL[2] - 8, 14];
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
    /// Read-only: SPEC-024a §7.5's mod log, with manifest scripting on.
    ModLog,
    /// Read-only: the world canvas's render path and its metrics
    /// (SPEC-022 §3.7, §3.12), after the spawner and the camera page.
    Renderer,
    /// The tabs reached by a press alone; F8 leaves them for the watch view.
    /// Decoded instructions, the call-stack filter and the debugger VM.
    Trace,
    /// The live timed queue and the armed explosives in it.
    Timers,
    /// The map teleporter list and the spawn buttons.
    World,
    /// Watch Mode's controls and the bot's status.
    Bot,
}
impl Page {
    fn next(self) -> Self {
        match self {
            Self::Watch => Self::Inspect,
            Self::Inspect => Self::Globals,
            Self::Globals => Self::MapVars,
            Self::MapVars => Self::MapJump,
            Self::MapJump => Self::Spawn,
            Self::Spawn | Self::Camera | Self::ModLog => Self::Renderer,
            Self::Renderer | Self::Trace | Self::Timers | Self::World | Self::Bot => Self::Watch,
        }
    }
    /// The page whose tab shows this page open: the tabs' own pages, and the
    /// watch view for the rest of the F8 cycle.
    pub(super) fn tab(self) -> Self {
        match self {
            Self::Inspect | Self::Trace | Self::Timers | Self::World | Self::Bot => self,
            _ => Self::Watch,
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Watch => "watch",
            Self::Inspect => "hex & object inspector",
            Self::Globals => "GVAR editor",
            Self::MapVars => "MVAR editor",
            Self::MapJump => "map jump",
            Self::Spawn => "item spawner",
            Self::Camera => "enhanced camera",
            Self::ModLog => "mod log",
            Self::Renderer => "renderer",
            Self::Trace => "trace & stepper",
            Self::Timers => "timers & explosives",
            Self::World => "world spawner & warp",
            Self::Bot => "autoplay bot",
        }
    }
    /// Whether the page shows the world's state as it runs, so a frame with
    /// no variable change can still change the panel.
    fn live(self) -> bool {
        matches!(self, Self::Trace | Self::Timers | Self::Bot)
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
    /// Its stepper holds the Step Into Critical setting.
    pub(super) inspector: Inspector,
    recent: VecDeque<String>,
    pub(crate) page: Page,
    /// The inspected object's map object id (stable across runs).
    pub(crate) selected: Option<u32>,
    /// The hex of the last map click, and its elevation.
    pub(crate) hex: Option<(i32, u8)>,
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
    /// The Trace tab's call-stack filter: hide trace entries with more than
    /// this many values on the return stack.
    pub(super) stack_limit: Option<usize>,
    /// Counts through [`super::debug_ui::PROCEDURES`].
    pub(super) procedure: usize,
    /// The World tab's spawn quantity.
    pub(super) quantity: u32,
    /// The game's key items, once a Keycard press has searched for them.
    pub(super) keys: Option<Vec<u32>>,
    /// The next of `keys` to spawn.
    pub(super) next_key: usize,
    /// What the open live page last showed.
    live: [u64; 3],
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
    pub(super) fn set_page(&mut self, page: Page) {
        self.page = page;
        self.cursor = page.first_row();
        self.entry.clear();
    }
    /// The typed entry as a signed value.
    pub(super) fn value(&self) -> Option<i32> {
        self.entry.parse().ok()
    }
    pub(super) fn queue(&mut self, command: DebugCommand) {
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
                    hex: None,
                    cursor: 0,
                    elevation: 0,
                    entry: String::new(),
                    outbox: Vec::new(),
                    note: String::new(),
                    stack_limit: None,
                    procedure: 0,
                    quantity: 1,
                    keys: None,
                    next_key: 0,
                    live: [0; 3],
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
        let live = if overlay.page.live() {
            [
                self.host.world.clock.ticks,
                overlay
                    .inspector
                    .live_entries(1)
                    .first()
                    .map_or(0, |entry| entry.seq + 1),
                self.watch
                    .as_ref()
                    .map_or(0, |watch| watch.agent.status().ticks),
            ]
        } else {
            [0; 3]
        };
        let moved = std::mem::replace(&mut overlay.live, live) != live;
        if frame.is_empty() {
            return moved;
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
        if !self.pointer.world && inside(PANEL, point) {
            self.debug_panel_press(point);
            return true;
        }
        let picked = self
            .object_at(point)
            .and_then(|object| self.host.world.get(object))
            .map(|object| object.raw.id);
        let hover = self.pointer_view().hover(point);
        let Some(overlay) = &mut self.debug else {
            return false;
        };
        overlay.selected = picked;
        overlay.hex = hover.hex.map(|hex| (hex, hover.elevation));
        overlay.set_page(Page::Inspect);
        overlay.note = match (picked, hover.hex) {
            (Some(id), _) => format!("selected object {id:#x}"),
            (None, Some(hex)) => format!("picked hex {hex}, no object"),
            (None, None) => "nothing under the pointer".into(),
        };
        self.thaw_view();
        true
    }
    /// A press on the open panel at frame pixel `ui`: its widget under the
    /// pixel acts ([`super::debug_ui`]), or says why it cannot, and any
    /// other spot does nothing. The panel takes the press either way.
    pub(crate) fn debug_panel_press(&mut self, ui: [i32; 2]) {
        let Some(overlay) = &self.debug else {
            return;
        };
        let pressed = self
            .debug_widgets(overlay)
            .into_iter()
            .find(|widget| inside(widget.rect, ui));
        let Some(widget) = pressed else {
            return;
        };
        match widget.blocked {
            Some(reason) => {
                self.set_debug_note(reason.to_owned());
                self.thaw_view();
                self.pointer.redraw = true;
            }
            None => self.debug_act(widget.action),
        }
    }
    pub(super) fn set_debug_note(&mut self, note: String) {
        if let Some(overlay) = &mut self.debug {
            overlay.note = note;
        }
    }
    /// Export the setup; the overlay's note on how it went.
    pub(super) fn export_note(&mut self) -> String {
        match self.export_setup() {
            Ok(folder) => format!("exported to {}", folder.display()),
            Err(error) => format!("refused: {error}"),
        }
    }
    /// Whether the overlay is open with its panel under frame pixel `ui`.
    /// On the unlocked world `click` asks with a canvas pixel, which is the
    /// pointer's own place there. That is no frame pixel, and a panel press
    /// never gets that far: the enhanced path takes those first.
    pub(crate) fn debug_panel_at(&self, ui: [i32; 2]) -> bool {
        let canvas = self.pointer.world && self.pointer.at == Some(ui);
        self.debug.is_some() && !canvas && inside(PANEL, ui)
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
            let scripted = self.host.mod_session.is_some();
            let next = match page {
                Page::Spawn if self.enhanced_view().is_some() => Page::Camera,
                Page::Spawn | Page::Camera if scripted => Page::ModLog,
                page => page.next(),
            };
            if let Some(overlay) = &mut self.debug {
                overlay.set_page(next);
            }
            self.thaw_view();
            return true;
        }
        // The read-only pages pass every other key through.
        if matches!(
            page,
            Page::Watch | Page::ModLog | Page::Renderer | Page::Trace | Page::Timers | Page::Bot
        ) {
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
        let warps = if page == Page::World {
            self.warp_maps()
        } else {
            Vec::new()
        };
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
                    Page::Watch
                    | Page::Camera
                    | Page::ModLog
                    | Page::Renderer
                    | Page::Trace
                    | Page::Timers
                    | Page::Bot => Err(typed),
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
                    Page::World => warps
                        .get(index)
                        .map(|map_number| DebugCommand::TeleportMap {
                            map_number: *map_number,
                            elevation: 0,
                            tile: None,
                        })
                        .ok_or("no map to warp to"),
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
            Page::Watch
            | Page::Camera
            | Page::ModLog
            | Page::Renderer
            | Page::Trace
            | Page::Timers
            | Page::Bot => 0,
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
            Page::World => self.warp_maps().len(),
        }
    }
    /// The inspected object's lines and its LVARs.
    pub(super) fn inspect_lines(&self, overlay: &DebugOverlay) -> (Vec<String>, Vec<(usize, i32)>) {
        let world = &self.host.world;
        let picked = overlay
            .hex
            .map(|(hex, elevation)| format!("picked hex {hex}  elevation {elevation}"));
        let Some(id) = overlay.selected else {
            let hint = "click a hex, door or critter on the map".to_owned();
            return (picked.into_iter().chain([hint]).collect(), Vec::new());
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
            object.prototype.as_ref().map_or_else(
                || "PRO none".to_owned(),
                |prototype| {
                    format!(
                        "PRO {:?}  flags {:#010x}  ext {:#x}",
                        prototype.info().kind,
                        prototype.flags.0,
                        prototype.extended.0
                    )
                },
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
        lines.extend(picked);
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
        if let Some(lock) = object.lock() {
            lines.push(format!(
                "lock {:#010x}{}{}{}",
                lock.0,
                if lock.contains(LockFlags::OPEN) {
                    " open"
                } else {
                    ""
                },
                if lock.contains(LockFlags::LOCKED) {
                    " locked"
                } else {
                    ""
                },
                if lock.contains(LockFlags::JAMMED) {
                    " jammed"
                } else {
                    ""
                },
            ));
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
        let mut column = Column::new(&painter);
        column.line(
            image,
            &format!("{} (F9 hides, F8 page)", overlay.page.title()),
            ColorKey::WHITE,
        );
        let (top, note_at) = Self::tab_layout(overlay.page);
        column.jump(top);
        // A window of rows around the cursor.
        let window = |count: usize| rows_around(overlay.cursor, count, LIST_ROWS);
        let marker = |row: usize| if row == overlay.cursor { ">" } else { " " };
        let entry = format!("{}_", overlay.entry);
        match overlay.page {
            Page::Watch => {
                column.line(image, "-- variable changes --", ColorKey::AMBER);
                for change in overlay.recent_changes() {
                    column.line(image, change.trim(), ColorKey::GREEN);
                }
                column.line(image, "-- script trace --", ColorKey::AMBER);
                for text in overlay.trace_lines(TRACE_LINES) {
                    column.line(image, &text, ColorKey::GREEN);
                }
            }
            Page::Inspect => {
                for text in &self.inspect_lines(overlay).0 {
                    column.line(image, text, ColorKey::GREEN);
                }
                if overlay.selected.is_some() {
                    let [heading, value] = Self::OBJECT_LINES;
                    column.jump(heading);
                    column.line(
                        image,
                        "-- LVARs (Enter sets, H full heal) --",
                        ColorKey::AMBER,
                    );
                    column.jump(value);
                    column.line(image, &format!("value: {entry}"), ColorKey::WHITE);
                }
            }
            Page::Globals | Page::MapVars => {
                let (label, values): (&str, &[i32]) = if overlay.page == Page::Globals {
                    ("GVAR", self.host.world.gvars.as_deref().unwrap_or(&[]))
                } else {
                    ("MVAR", &self.host.world.mvars)
                };
                column.line(image, "Up/Down choose, type, Enter sets", ColorKey::AMBER);
                if values.is_empty() {
                    column.line(image, &format!("no {label}s"), ColorKey::GREEN);
                }
                for index in window(values.len()) {
                    column.line(
                        image,
                        &format!("{} {label} {index} = {}", marker(index), values[index]),
                        ColorKey::GREEN,
                    );
                }
                column.line(image, &format!("value: {entry}"), ColorKey::WHITE);
            }
            Page::MapJump => {
                column.line(image, "Up/Down map, Left/Right elevation", ColorKey::AMBER);
                let tables = self.host.world.rules.tables();
                for number in window(MAP_COUNT).filter(|number| *number >= 1) {
                    column.line(
                        image,
                        &format!(
                            "{} {number:2} {}",
                            marker(number),
                            tables.map_name(number as i32).unwrap_or("?")
                        ),
                        ColorKey::GREEN,
                    );
                }
                column.line(
                    image,
                    &format!("elevation {}", overlay.elevation),
                    ColorKey::WHITE,
                );
                column.line(
                    image,
                    &format!("tile: {entry} (empty: entrance)"),
                    ColorKey::WHITE,
                );
            }
            Page::Camera => {
                column.line(
                    image,
                    "F free camera, C camera demo, E export",
                    ColorKey::AMBER,
                );
                let lines = self.camera_lines();
                for text in &lines {
                    column.line(image, text, ColorKey::GREEN);
                }
                // The button goes over the next lines; the note goes below it.
                for _ in lines.len()..6 {
                    column.line(image, "", ColorKey::WHITE);
                }
            }
            Page::ModLog => {
                let lines = self
                    .host
                    .mod_session
                    .as_ref()
                    .map_or_else(Vec::new, |session| session.log_lines());
                column.line(image, "-- newest last --", ColorKey::AMBER);
                let shown = lines.len().saturating_sub(LIST_ROWS);
                for text in &lines[shown..] {
                    column.line(image, text, ColorKey::GREEN);
                }
            }
            Page::Renderer => {
                column.line(image, "-- world canvas (SPEC-022) --", ColorKey::AMBER);
                for text in self.render_metrics().lines() {
                    column.line(image, &text, ColorKey::GREEN);
                }
            }
            Page::Spawn => {
                column.line(image, "type the PID (decimal), Enter", ColorKey::AMBER);
                let pid = overlay.entry.parse::<u32>().ok();
                column.line(
                    image,
                    &format!(
                        "PID: {entry} {}",
                        pid.map_or_else(String::new, |pid| format!("({pid:#010x})"))
                    ),
                    ColorKey::WHITE,
                );
                column.line(
                    image,
                    &format!("quantity: {} (Up/Down)", overlay.cursor),
                    ColorKey::WHITE,
                );
            }
            Page::Trace | Page::Timers | Page::World | Page::Bot => {
                let lines = match overlay.page {
                    Page::Trace => self.trace_lines_shown(overlay),
                    Page::Timers => self.timer_lines(),
                    Page::World => self.world_lines(overlay),
                    _ => self.bot_lines(),
                };
                for (text, key) in &lines {
                    column.line(image, text, *key);
                }
            }
        }
        for widget in self.debug_widgets(overlay) {
            widget.draw(&painter, image);
        }
        if !overlay.note.is_empty() {
            if let Some(y) = note_at {
                column.jump(y);
            }
            let key = if overlay.note.starts_with("refused") {
                ColorKey::RED
            } else {
                ColorKey::AMBER
            };
            column.line(image, &overlay.note, key);
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

    /// SPEC-024a §7.5: read-only, between Camera and Renderer, and only with
    /// manifest scripting on.
    #[test]
    fn the_mod_log_page_appears_with_manifest_scripting() {
        use vault_core::host_mods::ModSession;
        let dir = tempfile::tempdir().unwrap();
        let mut preview = world(dir.path());
        preview.set_debug(true);
        let cycle = |preview: &mut MapPreview| {
            let mut seen = Vec::new();
            for _ in 0..8 {
                preview.step(KeyCode::F8);
                seen.push(page(preview));
            }
            seen
        };
        assert!(!cycle(&mut preview).contains(&Page::ModLog));
        preview
            .host
            .enable_mod_scripts(std::sync::Arc::new(ModSession::new(Vec::new(), false)));
        while page(&preview) != Page::Spawn {
            preview.step(KeyCode::F8);
        }
        preview.step(KeyCode::F8);
        assert_eq!(page(&preview), Page::ModLog);
        assert!(!preview.debug_key(KeyCode::KeyA), "keys pass to the world");
        preview.image().unwrap();
        preview.step(KeyCode::F8);
        assert_eq!(page(&preview), Page::Renderer);
    }
}
