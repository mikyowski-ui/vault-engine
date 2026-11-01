//! The F9 overlay's widgets and its tabs, drawn in the overlay's style: black
//! panels, font 101 and one-pixel amber frames.
//!
//! A page lists its [`Widget`]s from the state it shows
//! ([`MapPreview::debug_widgets`]), and that one list is both drawn and hit
//! tested, so a press acts on what the panel shows. There is no release or
//! hover path: a widget acts when it is pressed. A button that cannot act now
//! is dimmed, and a press on it says why.
//!
//! A button that changes the world carries its [`DebugCommand`]s, which go to
//! the overlay's outbox and so into the log. Only what is not game state acts
//! at once: the panel's own settings and Watch Mode, whose changes the shell
//! logs by [`MapPreview::take_autoplay_change`].
use super::{
    MapPreview,
    bot::{BotSpeed, Watch},
    debug::{DebugOverlay, PANEL, Page},
};
use crate::interface::Painter;
use std::ops::Range;
use vault_core::{
    ScriptEvent, assets::WorldAssets, autoplay::AutoplayAgent, debug::DebugCommand,
    queue::EventKind, skills, ui::ColorKey, world::WorldObject, worldmap::MAP_COUNT,
};
use vault_debug::{
    DecodeMode, ObjectMutator, QueueEntryView, VmSnapshot, decode::decode_with, queue_entries,
};
use vault_formats::{
    map::{ObjectDetail, ObjectState},
    pro::{ItemSubtype, LockFlags, PrototypeKind},
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

const FONT: i32 = 101;
const LEFT: i32 = PANEL[0] + 4;
const WIDTH: i32 = PANEL[2] - 8;
const BUTTON_HEIGHT: i32 = 14;
const LINE: i32 = 10;
/// The tab strip, under the title line.
const TAB_Y: i32 = 11;
/// The first line under the tab strip, on every page.
pub(super) const TOP: i32 = TAB_Y + BUTTON_HEIGHT + 1;
/// Where the pages laid out by hand keep their note.
const NOTE_Y: i32 = 366;

/// Trace & Stepper: two rows of buttons, the debugger VM's three lines and a
/// heading, then the trace.
const TRACE_TEXT: i32 = TOP + 2 * (BUTTON_HEIGHT + 2);
const TRACE_ROWS: usize = 26;
/// How much of the live trace the call-stack filter looks through: the
/// inspector's whole ring.
const TRACE_WINDOW: usize = 256;
/// `VmStatus::CRITICAL`, the bit a critical section sets (SPEC-004 §3.3).
const VM_CRITICAL: u16 = 0x80;
/// The instructions a step looks through for the end of a critical section.
const CRITICAL_SCAN: u32 = 4096;

/// Hex & Object Inspector: up to eight lines of the object above these.
const LVAR_HEAD: i32 = 110;
const LVAR_TOP: i32 = LVAR_HEAD + LINE;
const LVAR_ROWS: usize = 18;
const VALUE_Y: i32 = LVAR_TOP + LVAR_ROWS as i32 * LINE + 1;
const OBJECT_BUTTONS: i32 = VALUE_Y + 13;
/// The procedures Trigger Proc and Begin go through: those
/// [`vault_core::debug::proc_event`] lets the debugger name.
pub(super) const PROCEDURES: [ScriptEvent; 5] = [
    ScriptEvent::Use,
    ScriptEvent::Talk,
    ScriptEvent::Damage,
    ScriptEvent::MapEnter,
    ScriptEvent::Destroy,
];

/// Timers & Explosives: the fast-forward button, then the queue.
const TIMER_TEXT: i32 = TOP + BUTTON_HEIGHT + 2;
const TIMER_ROWS: usize = 30;
/// The most `FastForwardTimer` commands one press queues.
const MAX_FAST_FORWARD: usize = 256;

/// World Spawner & Warp: two lines, the maps, then the buttons.
const WARP_TOP: i32 = TOP + 2 * LINE + 2;
const WARP_ROWS: usize = 26;
const WARP_BUTTON: i32 = WARP_TOP + WARP_ROWS as i32 * LINE + 2;
const SPAWN_BUTTONS: i32 = WARP_BUTTON + BUTTON_HEIGHT + 2;
/// The spawn quantities the Qty button goes through.
const QUANTITIES: [u32; 4] = [1, 5, 20, 100];
/// The item rows searched for keys (SPEC-003: a PID's low bytes are its row).
const ITEM_ROWS: u32 = 1024;

/// Autoplay Bot: two rows of buttons, then the bot's status.
const BOT_TEXT: i32 = TOP + 2 * (BUTTON_HEIGHT + 2);
/// Watch Mode's random stream, which only steers the bot: its actions are
/// logged, and a replay runs no agent.
const WATCH_SEED: i32 = 1;

/// The tab strip, left to right: its label and the page a press opens.
const TABS: [(&str, Page); 6] = [
    ("Pages", Page::Watch),
    ("Trace", Page::Trace),
    ("Object", Page::Inspect),
    ("Timers", Page::Timers),
    ("World", Page::World),
    ("Bot", Page::Bot),
];

/// `count` equal buttons side by side at height `y`.
fn cells<const COUNT: usize>(y: i32) -> [[i32; 4]; COUNT] {
    const GAP: i32 = 2;
    let count = COUNT as i32;
    let width = (WIDTH - GAP * (count - 1)) / count;
    std::array::from_fn(|k| [LEFT + k as i32 * (width + GAP), y, width, BUTTON_HEIGHT])
}

/// The rows of a list of `count` to show around `cursor`.
pub(super) fn rows_around(cursor: usize, count: usize, shown: usize) -> Range<usize> {
    let start = cursor
        .saturating_sub(shown / 2)
        .min(count.saturating_sub(shown));
    start..count.min(start + shown)
}

fn row_rect(top: i32, row: usize) -> [i32; 4] {
    [LEFT, top + row as i32 * LINE, WIDTH, LINE]
}

/// Whether `point` is in the `[x, y, width, height]` rectangle.
pub(super) fn inside([x, y, width, height]: [i32; 4], point: [i32; 2]) -> bool {
    (x..x + width).contains(&point[0]) && (y..y + height).contains(&point[1])
}

/// The panel's lines of text, from the top down.
pub(super) struct Column<'a> {
    painter: &'a Painter<'a>,
    y: i32,
}
impl<'a> Column<'a> {
    pub(super) fn new(painter: &'a Painter<'a>) -> Self {
        Self { painter, y: 0 }
    }
    pub(super) fn line(&mut self, image: &mut IndexedImage, text: &str, key: ColorKey) {
        let origin = [LEFT, self.y];
        self.painter
            .text(image, FONT, key, text.as_bytes(), origin, WIDTH);
        self.y += LINE;
    }
    pub(super) fn jump(&mut self, y: i32) {
        self.y = y;
    }
}

/// What a press on a widget does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Action {
    Show(Page),
    Export,
    /// Commit these to the outbox, in order.
    Queue(Vec<DebugCommand>),
    /// As the key does on this page.
    Key(KeyCode),
    /// Move the page's cursor to this row.
    Row(usize),
    NextProcedure,
    StepIntoCritical,
    StackFilter,
    StackLimit(isize),
    NextQuantity,
    SpawnKeycard,
    WatchStart,
    WatchPaused(bool),
    WatchStop,
    WatchSpeed(BotSpeed),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Button,
    Row,
}

/// One thing on the panel that takes a press.
#[derive(Debug, Clone)]
pub(super) struct Widget {
    pub(super) rect: [i32; 4],
    pub(super) label: String,
    pub(super) action: Action,
    /// A tab that is open, a setting that is on, the list's chosen row.
    pub(super) on: bool,
    /// Why it cannot act now.
    pub(super) blocked: Option<&'static str>,
    look: Look,
}
impl Widget {
    fn button(rect: [i32; 4], label: impl Into<String>, action: Action) -> Self {
        Self {
            rect,
            label: label.into(),
            action,
            on: false,
            blocked: None,
            look: Look::Button,
        }
    }
    fn row(rect: [i32; 4], label: impl Into<String>, row: usize, chosen: bool) -> Self {
        Self {
            on: chosen,
            look: Look::Row,
            ..Self::button(rect, label, Action::Row(row))
        }
    }
    fn on(self, on: bool) -> Self {
        Self { on, ..self }
    }
    fn blocked(self, blocked: Option<&'static str>) -> Self {
        Self { blocked, ..self }
    }
    pub(super) fn draw(&self, painter: &Painter, image: &mut IndexedImage) {
        let [x, y, width, height] = self.rect;
        let key = match self.look {
            Look::Row => {
                if self.on {
                    image.fill_rect(self.rect, painter.color(ColorKey::DARK_GREEN));
                }
                if self.on {
                    ColorKey::WHITE
                } else {
                    ColorKey::GREEN
                }
            }
            Look::Button => {
                let frame = if self.blocked.is_some() {
                    ColorKey::GREY
                } else {
                    ColorKey::AMBER
                };
                let fill = if self.on { frame } else { ColorKey::BLACK };
                image.fill_rect(self.rect, painter.color(frame));
                image.fill_rect([x + 1, y + 1, width - 2, height - 2], painter.color(fill));
                match (self.on, self.blocked) {
                    (true, _) => ColorKey::BLACK,
                    (false, Some(_)) => ColorKey::GREY,
                    (false, None) => ColorKey::WHITE,
                }
            }
        };
        let top = match self.look {
            Look::Row => y,
            Look::Button => y + 3,
        };
        painter.text(
            image,
            FONT,
            key,
            self.label.as_bytes(),
            [x + 2, top],
            width - 4,
        );
    }
}

/// Whether a queued event is an armed explosive's: a timely or a premature
/// detonation (SPEC-013 §3.10).
fn armed(entry: &QueueEntryView) -> bool {
    matches!(
        entry.kind,
        EventKind::Other(skills::EXPLOSION_EVENT | skills::PREMATURE_EVENT)
    )
}

/// How many `FastForwardTimer`s reach the soonest armed explosive. Each one
/// advances to the next due tick and runs what is due there, so that is one
/// for each due tick up to the detonation's.
fn steps_to_detonation(entries: &[QueueEntryView]) -> Option<usize> {
    let target = entries
        .iter()
        .filter(|entry| armed(entry))
        .map(|entry| entry.due)
        .min()?;
    let mut dues: Vec<u64> = entries
        .iter()
        .map(|entry| entry.due)
        .filter(|due| *due <= target)
        .collect();
    dues.dedup();
    Some(dues.len().min(MAX_FAST_FORWARD))
}

/// Whether the instruction after `word` may not be the next one in the code
/// (SPEC-004 §2.6): the jumps, branches, calls, returns and ends.
fn leaves_the_line(word: u16) -> bool {
    matches!(
        word,
        0x8004 | 0x8005 | 0x8010 | 0x8011 | 0x801c..=0x8024 | 0x802f | 0x8030
    )
}

/// The object's door flags, if it is a door.
fn door_flags(object: &WorldObject) -> Option<LockFlags> {
    match object.raw.state {
        ObjectState::Other {
            detail: ObjectDetail::Door(flags),
            ..
        } => Some(flags),
        _ => None,
    }
}

impl DebugOverlay {
    /// The trace entries the call-stack filter passes, the newest `count`,
    /// as lines. A `*` marks an instruction run in a critical section.
    fn filtered_trace(&self, count: usize) -> Vec<String> {
        let entries = self.inspector.live_entries(TRACE_WINDOW);
        let shown: Vec<_> = entries
            .iter()
            .filter(|entry| {
                self.stack_limit
                    .is_none_or(|limit| entry.return_depth <= limit)
            })
            .collect();
        let skip = shown.len().saturating_sub(count);
        shown[skip..]
            .iter()
            .map(|entry| {
                let instruction = entry
                    .instruction
                    .as_ref()
                    .map_or_else(|| "<undecodable>".to_owned(), |i| i.text());
                let critical = (entry.status_before | entry.status_after) & VM_CRITICAL != 0;
                format!(
                    "{}vm{} {:#06x} rs{} {instruction}",
                    if critical { "*" } else { " " },
                    entry.vm,
                    entry.pc,
                    entry.return_depth,
                )
            })
            .collect()
    }
    /// The procedure Trigger Proc and Begin name.
    fn procedure(&self) -> ScriptEvent {
        PROCEDURES[self.procedure % PROCEDURES.len()]
    }
}

impl MapPreview {
    /// The inspected object, while it exists.
    fn inspected(&self, overlay: &DebugOverlay) -> Option<&WorldObject> {
        let world = &self.host.world;
        world
            .reference_for_id(overlay.selected?)
            .and_then(|reference| world.get(reference))
    }
    /// The numbered maps this game can load, in order.
    pub(super) fn warp_maps(&self) -> Vec<i32> {
        let assets = &self.host.assets;
        (1..MAP_COUNT as i32)
            .filter(|number| {
                assets
                    .map_path(*number)
                    .is_some_and(|path| assets.available(path.as_bytes()))
            })
            .collect()
    }
    /// The instructions a Step runs: one, or with Step Into Critical off, a
    /// critical section the next instruction enters or is inside, up to its
    /// end. That is read along the code, which the VM may leave, so the count
    /// stops at the first instruction that can go elsewhere and a section that
    /// branches takes another press. It never runs on past where it can see.
    /// `VmStep` itself stops inside sections, which only the debugger's freeze
    /// makes safe.
    fn step_count(&self, overlay: &DebugOverlay) -> u32 {
        let Some(vm) = self
            .host
            .debug_vm
            .as_ref()
            .filter(|_| !overlay.inspector.stepper.step_into_critical)
        else {
            return 1;
        };
        let mode = DecodeMode::of(&vm.program);
        let mut inside = vm.status.0 & VM_CRITICAL != 0;
        let mut at = vm.pc;
        for count in 1..=CRITICAL_SCAN {
            let Ok(instruction) = decode_with(vm.code(), at, mode) else {
                return count.saturating_sub(1).max(1);
            };
            match instruction.word {
                0x8002 | 0x804a => inside = true,
                0x8003 | 0x804b if inside => return count,
                _ if !inside || leaves_the_line(instruction.word) => return count,
                _ => {}
            }
            at = instruction.next_address();
        }
        CRITICAL_SCAN
    }

    /// Everything on the panel that takes a press: the tabs, then the page's.
    pub(super) fn debug_widgets(&self, overlay: &DebugOverlay) -> Vec<Widget> {
        let tab_rects = cells::<{ TABS.len() }>(TAB_Y);
        let mut widgets: Vec<Widget> = TABS
            .iter()
            .zip(tab_rects)
            .map(|((label, page), rect)| {
                Widget::button(rect, *label, Action::Show(*page))
                    .on(page.tab() == overlay.page.tab())
            })
            .collect();
        match overlay.page {
            Page::Camera => widgets.push(Widget::button(
                super::debug::EXPORT_BUTTON,
                "Export Current Setup as Mod (E)",
                Action::Export,
            )),
            Page::Trace => self.trace_widgets(overlay, &mut widgets),
            Page::Inspect => self.object_widgets(overlay, &mut widgets),
            Page::Timers => self.timer_widgets(&mut widgets),
            Page::World => self.world_widgets(overlay, &mut widgets),
            Page::Bot => self.bot_widgets(&mut widgets),
            _ => {}
        }
        widgets
    }

    fn trace_widgets(&self, overlay: &DebugOverlay, widgets: &mut Vec<Widget>) {
        let running = self.host.debug_vm.is_some();
        let paused = self.host.debug_paused;
        let not_running = (!running).then_some("press Begin first");
        let [begin, step, abort, pause] = cells::<4>(TOP);
        let event = overlay.procedure().code();
        let start = overlay
            .selected
            .map(|object| DebugCommand::VmBegin { object, event });
        let cannot_start = if running {
            Some("a procedure is being stepped")
        } else if start.is_none() {
            Some("pick an object first")
        } else {
            None
        };
        widgets.extend([
            Widget::button(begin, "Begin", Action::Queue(start.into_iter().collect()))
                .blocked(cannot_start),
            Widget::button(
                step,
                "Step",
                Action::Queue(vec![DebugCommand::VmStep {
                    count: self.step_count(overlay),
                }]),
            )
            .blocked(not_running),
            Widget::button(abort, "Abort", Action::Queue(vec![DebugCommand::VmAbort]))
                .blocked(not_running),
            Widget::button(
                pause,
                if paused { "Resume" } else { "Pause" },
                Action::Queue(vec![DebugCommand::SetPaused { on: !paused }]),
            )
            .on(paused)
            .blocked(running.then_some("the stepped procedure holds the world")),
        ]);
        let y = TOP + BUTTON_HEIGHT + 2;
        let critical = overlay.inspector.stepper.step_into_critical;
        let stack = match overlay.stack_limit {
            Some(limit) => format!("Stack <= {limit}"),
            None => "Stack: all".to_owned(),
        };
        let off = overlay.stack_limit.is_none().then_some("the filter is off");
        widgets.extend([
            Widget::button(
                [LEFT, y, 154, BUTTON_HEIGHT],
                format!(
                    "Step Into Critical: {}",
                    if critical { "on" } else { "off" }
                ),
                Action::StepIntoCritical,
            )
            .on(critical),
            Widget::button(
                [LEFT + 156, y, 72, BUTTON_HEIGHT],
                stack,
                Action::StackFilter,
            )
            .on(overlay.stack_limit.is_some()),
            Widget::button(
                [LEFT + 230, y, 24, BUTTON_HEIGHT],
                "-",
                Action::StackLimit(-1),
            )
            .blocked(off),
            Widget::button(
                [LEFT + 256, y, 24, BUTTON_HEIGHT],
                "+",
                Action::StackLimit(1),
            )
            .blocked(off),
        ]);
    }
    /// The Trace & Stepper tab's lines.
    pub(super) fn trace_lines_shown(&self, overlay: &DebugOverlay) -> Vec<(String, ColorKey)> {
        let mut lines = match &self.host.debug_vm {
            Some(vm) => {
                let snapshot = VmSnapshot::of(vm);
                let next = match &snapshot.next {
                    Ok(instruction) => instruction.text(),
                    Err(error) => error.to_string(),
                };
                let calls: Vec<String> = snapshot
                    .returns
                    .iter()
                    .rev()
                    .take(8)
                    .map(|value| format!("{:#x}", value.payload))
                    .collect();
                let critical = snapshot.status & VM_CRITICAL != 0;
                vec![
                    (
                        format!(
                            "stepping vm {} at {:#06x}{}",
                            snapshot.id,
                            snapshot.pc,
                            if critical { " (critical)" } else { "" }
                        ),
                        ColorKey::WHITE,
                    ),
                    (format!("next: {next}"), ColorKey::WHITE),
                    (format!("calls: {}", calls.join(" ")), ColorKey::GREEN),
                ]
            }
            None => vec![(
                format!("Begin runs {}", overlay.procedure_name()),
                ColorKey::GREEN,
            )],
        };
        lines.push(("-- script trace (* critical) --".into(), ColorKey::AMBER));
        lines.extend(
            overlay
                .filtered_trace(TRACE_ROWS)
                .into_iter()
                .map(|text| (text, ColorKey::GREEN)),
        );
        lines
    }

    fn object_widgets(&self, overlay: &DebugOverlay, widgets: &mut Vec<Widget>) {
        let locals = self.inspect_lines(overlay).1;
        let shown = rows_around(overlay.cursor, locals.len(), LVAR_ROWS);
        let first = shown.start;
        for index in shown {
            let (_, value) = locals[index];
            let chosen = index == overlay.cursor;
            widgets.push(Widget::row(
                row_rect(LVAR_TOP, index - first),
                format!("{} LVAR {index} = {value}", if chosen { ">" } else { " " }),
                index,
                chosen,
            ));
        }
        let object = self.inspected(overlay);
        let id = overlay.selected.map(ObjectMutator);
        let world = &self.host.world;
        let critter = object.is_some_and(|o| matches!(o.raw.state, ObjectState::Critter(_)));
        let dead = overlay
            .selected
            .and_then(|id| world.reference_for_id(id))
            .is_some_and(|reference| world.is_dead(reference));
        let cannot_hurt = if !critter {
            Some("not a critter")
        } else if dead {
            Some("it is dead")
        } else {
            None
        };
        let unscripted = object
            .and_then(|o| u32::try_from(o.raw.script_id).ok())
            .is_none_or(|sid| !world.scripts.contains_key(&sid));
        let door = object.and_then(door_flags);
        let shut = door.is_some_and(|flags| {
            flags.contains(LockFlags::LOCKED) || flags.contains(LockFlags::JAMMED)
        });
        let door_command = door.zip(id).map(|(flags, id)| {
            if shut {
                id.door(None, Some(false), Some(false))
            } else {
                id.door(Some(!flags.contains(LockFlags::OPEN)), None, None)
            }
        });
        let door_label = match door {
            Some(_) if shut => "Unlock Door",
            Some(_) => "Toggle Door",
            None => "Unlock/Toggle Door",
        };
        let event = overlay.procedure().code();
        let fixed = overlay.value().unwrap_or(0);
        let [procedure, trigger] = cells::<2>(OBJECT_BUTTONS);
        let [toggle, edit] = cells::<2>(OBJECT_BUTTONS + BUTTON_HEIGHT + 2);
        let [heal, kill] = cells::<2>(OBJECT_BUTTONS + 2 * (BUTTON_HEIGHT + 2));
        let no_object = object.is_none().then_some("click an object on the map");
        widgets.extend([
            Widget::button(
                procedure,
                format!("Proc: {}", overlay.procedure_name()),
                Action::NextProcedure,
            ),
            Widget::button(
                trigger,
                "Trigger Proc",
                Action::Queue(id.iter().map(|id| id.fire(event, fixed)).collect()),
            )
            .blocked(no_object.or(unscripted.then_some("it has no script"))),
            Widget::button(
                toggle,
                door_label,
                Action::Queue(door_command.into_iter().collect()),
            )
            .blocked(no_object.or(door.is_none().then_some("not a door"))),
            Widget::button(edit, "Edit Vars", Action::Key(KeyCode::Enter))
                .blocked(no_object.or(locals.is_empty().then_some("it has no LVARs"))),
            Widget::button(
                heal,
                "Heal",
                Action::Queue(
                    overlay
                        .selected
                        .map(|object| DebugCommand::HealCritter { object })
                        .into_iter()
                        .collect(),
                ),
            )
            .blocked(no_object.or(cannot_hurt)),
            Widget::button(
                kill,
                "Kill",
                Action::Queue(id.iter().map(|id| id.hp(0)).collect()),
            )
            .blocked(no_object.or(cannot_hurt)),
        ]);
    }

    fn timer_widgets(&self, widgets: &mut Vec<Widget>) {
        let steps = steps_to_detonation(&queue_entries(&self.host.world));
        let world = &self.host.world;
        let blocked = if world.combat || world.dialogue.open {
            Some("the queue does not run in combat or dialogue")
        } else if steps.is_none() {
            Some("no armed explosive is queued")
        } else {
            None
        };
        widgets.push(
            Widget::button(
                [LEFT, TOP, WIDTH, BUTTON_HEIGHT],
                "[Fast-Forward to Detonation]",
                Action::Queue(vec![DebugCommand::FastForwardTimer; steps.unwrap_or(0)]),
            )
            .blocked(blocked),
        );
    }
    /// The Timers & Explosives tab's lines: armed explosives first and in red,
    /// then the rest of the queue, each in the order it will run.
    pub(super) fn timer_lines(&self) -> Vec<(String, ColorKey)> {
        let world = &self.host.world;
        let now = world.clock.ticks;
        let (fuses, rest): (Vec<_>, Vec<_>) = queue_entries(world).into_iter().partition(armed);
        let mut lines = vec![(
            format!(
                "tick {now}  {} queued, {} armed",
                fuses.len() + rest.len(),
                fuses.len()
            ),
            ColorKey::AMBER,
        )];
        let total = fuses.len() + rest.len();
        let shown = total.min(TIMER_ROWS);
        for entry in fuses.iter().chain(&rest).take(shown) {
            let ticks = entry.due.saturating_sub(now);
            let owner = entry
                .owner
                .map_or_else(|| "-".to_owned(), |id| format!("{id:#x}"));
            let key = if armed(entry) {
                ColorKey::RED
            } else {
                ColorKey::GREEN
            };
            lines.push((
                format!("{:>6}.{}s {owner} {}", ticks / 10, ticks % 10, entry.detail),
                key,
            ));
        }
        if total > shown {
            lines.push((format!("... {} more", total - shown), ColorKey::GREEN));
        }
        lines
    }

    fn world_widgets(&self, overlay: &DebugOverlay, widgets: &mut Vec<Widget>) {
        let maps = self.warp_maps();
        let tables = self.host.world.rules.tables();
        let shown = rows_around(overlay.cursor, maps.len(), WARP_ROWS);
        let first = shown.start;
        for row in shown {
            let number = maps[row];
            let chosen = row == overlay.cursor;
            widgets.push(Widget::row(
                row_rect(WARP_TOP, row - first),
                format!(
                    "{} {number:2} {}",
                    if chosen { ">" } else { " " },
                    tables.map_name(number).unwrap_or("?")
                ),
                row,
                chosen,
            ));
        }
        let quantity = overlay.quantity;
        let spawn = |pid: u32| Action::Queue(vec![DebugCommand::SpawnItem { pid, quantity }]);
        let typed = overlay.entry.parse::<u32>().ok();
        let [item, c4, keycard, count] = cells::<4>(SPAWN_BUTTONS);
        widgets.extend([
            Widget::button(
                [LEFT, WARP_BUTTON, WIDTH, BUTTON_HEIGHT],
                "Warp to Map",
                Action::Key(KeyCode::Enter),
            )
            .blocked(maps.is_empty().then_some("no map to warp to")),
            Widget::button(item, "Item", spawn(typed.unwrap_or(0)))
                .blocked(typed.is_none().then_some("type a PID first")),
            Widget::button(c4, "C4", spawn(skills::PLASTIC)),
            Widget::button(keycard, "Keycard", Action::SpawnKeycard),
            Widget::button(count, format!("Qty: {quantity}"), Action::NextQuantity),
        ]);
    }
    /// The World Spawner & Warp tab's lines, above its list of maps.
    pub(super) fn world_lines(&self, overlay: &DebugOverlay) -> Vec<(String, ColorKey)> {
        vec![
            (
                "Up/Down or click a map, Enter warps".into(),
                ColorKey::AMBER,
            ),
            (format!("item PID: {}_", overlay.entry), ColorKey::WHITE),
        ]
    }

    fn bot_widgets(&self, widgets: &mut Vec<Widget>) {
        let watching = self.watch.is_some();
        let off = (!watching).then_some("Watch Mode is off");
        let [start, pause, resume, stop] = cells::<4>(TOP);
        let paused = self.watch.as_ref().is_some_and(|watch| watch.paused);
        widgets.extend([
            Widget::button(start, "Start", Action::WatchStart)
                .on(watching)
                .blocked(watching.then_some("Watch Mode is on")),
            Widget::button(pause, "Pause", Action::WatchPaused(true))
                .on(paused)
                .blocked(off),
            Widget::button(resume, "Resume", Action::WatchPaused(false)).blocked(off),
            Widget::button(stop, "Stop", Action::WatchStop).blocked(off),
        ]);
        let speeds = cells::<{ BotSpeed::ALL.len() }>(TOP + BUTTON_HEIGHT + 2);
        widgets.extend(BotSpeed::ALL.into_iter().zip(speeds).map(|(speed, rect)| {
            Widget::button(rect, speed.label(), Action::WatchSpeed(speed))
                .on(self
                    .watch
                    .as_ref()
                    .is_some_and(|watch| watch.speed == speed))
                .blocked(off)
        }));
    }
    /// The Autoplay Bot tab's lines.
    pub(super) fn bot_lines(&self) -> Vec<(String, ColorKey)> {
        let Some(watch) = &self.watch else {
            return vec![("Watch Mode is off".into(), ColorKey::GREEN)];
        };
        let status = watch.agent.status();
        let mut lines = vec![(
            format!(
                "Watch Mode {} at {}",
                if watch.paused { "paused" } else { "playing" },
                watch.speed.label()
            ),
            ColorKey::WHITE,
        )];
        let text = [
            format!("goal: {}", status.goal),
            format!("tactic: {}", status.tactic),
            format!("ticks {}  actions {}", status.ticks, status.actions),
            format!("refusals {}  stuck {}", status.refusals, status.stuck),
            format!(
                "target hex {}",
                status
                    .target_hex
                    .map_or_else(|| "-".to_owned(), |hex| hex.to_string())
            ),
        ];
        lines.extend(text.map(|text| (text, ColorKey::GREEN)));
        if let Some(violation) = &status.violation {
            lines.push((format!("violation: {violation}"), ColorKey::RED));
        }
        lines
    }

    /// Where a page's lines start, and whether its note has a fixed place.
    pub(super) fn tab_layout(page: Page) -> (i32, Option<i32>) {
        match page {
            Page::Trace => (TRACE_TEXT, Some(NOTE_Y)),
            Page::Timers => (TIMER_TEXT, Some(NOTE_Y)),
            Page::World | Page::Inspect => (TOP, Some(NOTE_Y)),
            Page::Bot => (BOT_TEXT, Some(NOTE_Y)),
            _ => (TOP, None),
        }
    }
    /// The Object tab's fixed places: the LVAR heading and the typed value.
    pub(super) const OBJECT_LINES: [i32; 2] = [LVAR_HEAD, VALUE_Y];

    /// A press on a widget.
    pub(super) fn debug_act(&mut self, action: Action) {
        match action {
            Action::Export => {
                let note = self.export_note();
                self.set_debug_note(note);
            }
            Action::Key(key) => {
                self.debug_key(key);
            }
            Action::SpawnKeycard => self.spawn_keycard(),
            Action::WatchStart => match AutoplayAgent::new(WATCH_SEED, false) {
                Ok(agent) => self.watch = Some(Box::new(Watch::new(agent))),
                Err(error) => self.set_debug_note(format!("refused: {error}")),
            },
            Action::WatchPaused(paused) => {
                if let Some(watch) = &mut self.watch {
                    watch.paused = paused;
                }
            }
            Action::WatchStop => self.watch = None,
            Action::WatchSpeed(speed) => {
                if let Some(watch) = &mut self.watch {
                    watch.speed = speed;
                }
            }
            other => {
                if let Some(overlay) = &mut self.debug {
                    overlay.act(other);
                }
            }
        }
        self.thaw_view();
        self.pointer.redraw = true;
    }
    /// Spawn the next key the game has, so each press gives another. Keys are
    /// the Key subtype of item (SPEC-003): no specification names a keycard's
    /// PID, so the prototypes are searched, once, on the first press.
    fn spawn_keycard(&mut self) {
        let assets = &self.host.assets;
        let Some(overlay) = &mut self.debug else {
            return;
        };
        let keys = overlay.keys.get_or_insert_with(|| {
            (1..ITEM_ROWS)
                .filter(|pid| {
                    assets.prototype(*pid).is_ok_and(|prototype| {
                        prototype.info().kind == PrototypeKind::Item(ItemSubtype::Key)
                    })
                })
                .collect()
        });
        match keys.get(overlay.next_key % keys.len().max(1)).copied() {
            Some(pid) => {
                overlay.next_key += 1;
                overlay.queue(DebugCommand::SpawnItem {
                    pid,
                    quantity: overlay.quantity,
                });
            }
            None => overlay.note = "this game has no key items".into(),
        }
    }
}

impl DebugOverlay {
    /// The name of the procedure Trigger Proc and Begin name.
    fn procedure_name(&self) -> String {
        String::from_utf8_lossy(self.procedure().name()).into_owned()
    }
    /// A press that only concerns the panel, or commits commands.
    fn act(&mut self, action: Action) {
        match action {
            Action::Show(page) => self.set_page(page),
            Action::Queue(commands) => {
                for command in commands {
                    self.queue(command);
                }
                self.entry.clear();
            }
            Action::Row(row) => self.cursor = row,
            Action::NextProcedure => self.procedure += 1,
            Action::StepIntoCritical => {
                let stepper = &mut self.inspector.stepper;
                stepper.step_into_critical = !stepper.step_into_critical;
            }
            Action::StackFilter => {
                self.stack_limit = match self.stack_limit {
                    Some(_) => None,
                    None => Some(0),
                };
            }
            Action::StackLimit(change) => {
                self.stack_limit = self
                    .stack_limit
                    .map(|limit| limit.saturating_add_signed(change));
            }
            Action::NextQuantity => {
                let at = QUANTITIES.iter().position(|q| *q == self.quantity);
                self.quantity = QUANTITIES[at.map_or(0, |at| (at + 1) % QUANTITIES.len())];
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        enhanced::{EnhancedSettings, PointerAt, PointerButton},
        map_preview::{
            fixture, state_fixture,
            test_support::{interactive, load, synthetic_corpus},
        },
    };
    use vault_core::debug::proc_event;
    use vault_fixtures::campaign::{self, Edition};
    use vault_render::layers::EnhancedLayout;

    /// The first campaign map with the overlay open.
    fn campaign_world(root: &std::path::Path) -> MapPreview {
        campaign::write_campaign(root, Edition::Fallout1);
        open(root)
    }
    fn open(root: &std::path::Path) -> MapPreview {
        let mut preview = load(root, "maps/synthetic.map");
        preview
            .host
            .load_map(&campaign::start(Edition::Fallout1))
            .unwrap();
        preview.tick(16).unwrap();
        preview.image().unwrap();
        preview.set_debug(true);
        preview
    }
    fn widget(preview: &MapPreview, label: &str) -> Widget {
        let overlay = preview.debug.as_ref().unwrap();
        preview
            .debug_widgets(overlay)
            .into_iter()
            .find(|widget| widget.label.starts_with(label))
            .unwrap_or_else(|| panic!("no {label} on {:?}", overlay.page))
    }
    /// A press and release on the widget, as the mouse gives them.
    fn press(preview: &mut MapPreview, label: &str) {
        let [x, y, width, height] = widget(preview, label).rect;
        let point = [x + width / 2, y + height / 2];
        assert!(preview.click(point, false), "{label} took the press");
        preview.release(Some(point), false);
    }
    fn page(preview: &MapPreview) -> Page {
        preview.debug.as_ref().unwrap().page
    }
    fn note(preview: &MapPreview) -> String {
        preview.debug.as_ref().unwrap().note.clone()
    }
    fn inspect(preview: &mut MapPreview, id: u32) {
        let overlay = preview.debug.as_mut().unwrap();
        overlay.selected = Some(id);
        overlay.set_page(Page::Inspect);
    }
    fn reference(preview: &MapPreview, id: u32) -> u32 {
        preview.host.world.reference_for_id(id).unwrap()
    }
    fn apply(preview: &mut MapPreview) {
        for command in preview.take_debug_commands() {
            preview.apply_debug(&command).unwrap();
        }
    }

    #[test]
    fn each_tab_opens_on_a_press_and_f8_keeps_its_cycle() {
        let dir = synthetic_corpus();
        let mut preview = campaign_world(dir.path());
        assert_eq!(page(&preview), Page::Watch);
        for (label, target) in TABS {
            press(&mut preview, label);
            assert_eq!(page(&preview), target, "{label}");
            assert!(widget(&preview, label).on, "{label} shows open");
            preview.image().unwrap();
        }
        assert!(preview.take_debug_commands().is_empty(), "tabs only show");
        // F8 leaves a tab for the watch view, then cycles as it always has.
        press(&mut preview, "Bot");
        preview.step(KeyCode::F8);
        assert_eq!(page(&preview), Page::Watch);
        let mut seen = vec![page(&preview)];
        for _ in 0..6 {
            preview.step(KeyCode::F8);
            seen.push(page(&preview));
        }
        assert_eq!(
            seen,
            [
                Page::Watch,
                Page::Inspect,
                Page::Globals,
                Page::MapVars,
                Page::MapJump,
                Page::Spawn,
                Page::Renderer
            ]
        );
        // Those pages are under the first tab, but for the inspector's own.
        assert!(widget(&preview, "Pages").on);
    }

    #[test]
    fn every_widget_fits_the_panel_without_overlapping() {
        let dir = synthetic_corpus();
        let mut preview = campaign_world(dir.path());
        inspect(&mut preview, campaign::FRIEND);
        for target in [
            Page::Watch,
            Page::Inspect,
            Page::Camera,
            Page::Trace,
            Page::Timers,
            Page::World,
            Page::Bot,
        ] {
            preview.debug.as_mut().unwrap().set_page(target);
            let widgets = preview.debug_widgets(preview.debug.as_ref().unwrap());
            for (k, widget) in widgets.iter().enumerate() {
                let [x, y, width, height] = widget.rect;
                assert!(
                    x >= PANEL[0]
                        && y >= PANEL[1]
                        && x + width <= PANEL[0] + PANEL[2]
                        && y + height <= PANEL[1] + PANEL[3],
                    "{} on {target:?}",
                    widget.label
                );
                for other in &widgets[k + 1..] {
                    let [a, b, c, d] = other.rect;
                    assert!(
                        x + width <= a || a + c <= x || y + height <= b || b + d <= y,
                        "{} and {} on {target:?}",
                        widget.label,
                        other.label
                    );
                }
            }
        }
        const {
            assert!(NOTE_Y + LINE <= PANEL[3]);
            assert!(SPAWN_BUTTONS + BUTTON_HEIGHT <= NOTE_Y);
            assert!(OBJECT_BUTTONS + 3 * BUTTON_HEIGHT + 4 <= NOTE_Y);
        }
    }

    #[test]
    fn a_map_click_keeps_the_hex_with_the_object() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.tick(16).unwrap();
        preview.image().unwrap();
        preview.set_debug(true);
        let npc = [336, 170];
        assert!(preview.click(npc, false));
        preview.release(Some(npc), false);
        let hex = preview.view.grid.pick_hex(npc).unwrap();
        let overlay = preview.debug.as_ref().unwrap();
        assert_eq!(page(&preview), Page::Inspect);
        assert!(overlay.selected.is_some());
        assert_eq!(overlay.hex, Some((hex, 0)));
        let lines = preview.inspect_lines(overlay).0;
        assert!(lines.iter().any(|line| line.starts_with("PRO Critter")));
        assert!(lines.iter().any(|line| line.contains("script 0x04000001")));
        assert!(lines.iter().any(|line| line.contains("flags 0x")));
        assert!(
            lines
                .iter()
                .any(|line| *line == format!("picked hex {hex}  elevation 0"))
        );
        // A bare hex is picked too, with the object cleared.
        assert!(preview.click([10, 10], false));
        let overlay = preview.debug.as_ref().unwrap();
        assert_eq!(overlay.selected, None);
        let hex = preview.view.grid.pick_hex([10, 10]).unwrap();
        assert_eq!(overlay.hex, Some((hex, 0)));
        assert_eq!(note(&preview), format!("picked hex {hex}, no object"));
    }

    #[test]
    fn the_door_button_unlocks_then_toggles() {
        let dir = synthetic_corpus();
        let mut preview = campaign_world(dir.path());
        inspect(&mut preview, campaign::LOCKED_DOOR);
        let lines = preview.inspect_lines(preview.debug.as_ref().unwrap()).0;
        assert!(
            lines.iter().any(|line| line.contains(" locked")),
            "{lines:?}"
        );
        assert!(lines.iter().any(|line| line.contains("Scenery(Door)")));
        press(&mut preview, "Unlock Door");
        let commands = preview.take_debug_commands();
        assert_eq!(
            commands,
            [DebugCommand::SetDoor {
                object: campaign::LOCKED_DOOR,
                open: None,
                locked: Some(false),
                jammed: Some(false),
            }]
        );
        preview.apply_debug(&commands[0]).unwrap();
        press(&mut preview, "Toggle Door");
        let commands = preview.take_debug_commands();
        assert_eq!(
            commands,
            [DebugCommand::SetDoor {
                object: campaign::LOCKED_DOOR,
                open: Some(true),
                locked: None,
                jammed: None,
            }]
        );
        preview.apply_debug(&commands[0]).unwrap();
        press(&mut preview, "Toggle Door");
        assert_eq!(
            preview.take_debug_commands(),
            [DebugCommand::SetDoor {
                object: campaign::LOCKED_DOOR,
                open: Some(false),
                locked: None,
                jammed: None,
            }]
        );
        // The jammed door takes the same button.
        inspect(&mut preview, campaign::JAMMED_DOOR);
        press(&mut preview, "Unlock Door");
        assert_eq!(preview.take_debug_commands().len(), 1);
    }

    #[test]
    fn a_blocked_button_says_why_and_queues_nothing() {
        let dir = synthetic_corpus();
        let mut preview = campaign_world(dir.path());
        press(&mut preview, "Object");
        for (label, reason) in [
            ("Unlock/Toggle Door", "click an object on the map"),
            ("Trigger Proc", "click an object on the map"),
            ("Edit Vars", "click an object on the map"),
            ("Heal", "click an object on the map"),
            ("Kill", "click an object on the map"),
        ] {
            press(&mut preview, label);
            assert_eq!(note(&preview), reason);
        }
        inspect(&mut preview, campaign::PLAIN_DOOR);
        press(&mut preview, "Heal");
        assert_eq!(note(&preview), "not a critter");
        press(&mut preview, "Trigger Proc");
        assert_eq!(note(&preview), "it has no script");
        inspect(&mut preview, campaign::FOE);
        press(&mut preview, "Unlock/Toggle Door");
        assert_eq!(note(&preview), "not a door");
        assert!(preview.take_debug_commands().is_empty());
    }

    #[test]
    fn the_object_buttons_queue_what_they_name() {
        let dir = synthetic_corpus();
        let mut preview = campaign_world(dir.path());
        // Every procedure on the list is one the debugger may name.
        for event in PROCEDURES {
            assert_eq!(proc_event(event.code()), Some(event));
        }
        inspect(&mut preview, campaign::SCRIPTED_DOOR);
        press(&mut preview, "Proc: use_p_proc");
        press(&mut preview, "Proc: talk_p_proc");
        press(&mut preview, "Trigger Proc");
        assert_eq!(
            preview.take_debug_commands(),
            [DebugCommand::FireProc {
                object: campaign::SCRIPTED_DOOR,
                event: 14,
                fixed: 0,
            }]
        );
        // A typed value is the fixed parameter, and goes with the press.
        preview.step(KeyCode::Digit6);
        press(&mut preview, "Trigger Proc");
        assert_eq!(
            preview.take_debug_commands(),
            [DebugCommand::FireProc {
                object: campaign::SCRIPTED_DOOR,
                event: 14,
                fixed: 6,
            }]
        );
        assert!(preview.debug.as_ref().unwrap().entry.is_empty());
        // A row press chooses the LVAR Edit Vars sets.
        inspect(&mut preview, campaign::FRIEND);
        press(&mut preview, "  LVAR 2");
        assert_eq!(preview.debug.as_ref().unwrap().cursor, 2);
        press(&mut preview, "Edit Vars");
        assert_eq!(note(&preview), "type a number first");
        for key in [KeyCode::Digit7, KeyCode::Minus] {
            preview.step(key);
        }
        press(&mut preview, "Edit Vars");
        let commands = preview.take_debug_commands();
        assert_eq!(
            commands,
            [DebugCommand::SetObjectLocalVar {
                object: campaign::FRIEND,
                index: 2,
                value: -7,
            }]
        );
        preview.apply_debug(&commands[0]).unwrap();
        assert!(widget(&preview, "> LVAR 2 = -7").on);
        press(&mut preview, "Heal");
        press(&mut preview, "Kill");
        assert_eq!(
            preview.take_debug_commands(),
            [
                DebugCommand::HealCritter {
                    object: campaign::FRIEND,
                },
                DebugCommand::SetHitPoints {
                    object: campaign::FRIEND,
                    value: 0,
                },
            ]
        );
    }

    #[test]
    fn the_timers_tab_fast_forwards_to_the_detonation() {
        let dir = synthetic_corpus();
        let mut preview = campaign_world(dir.path());
        press(&mut preview, "Timers");
        press(&mut preview, "[Fast-Forward");
        assert_eq!(note(&preview), "no armed explosive is queued");
        assert!(preview.take_debug_commands().is_empty());
        // Armed dynamite, with one event before it and one tied with it.
        let dynamite = reference(&preview, campaign::DYNAMITE);
        let now = preview.host.world.clock.ticks;
        let queue = &mut preview.host.world.queue;
        queue
            .insert(
                now,
                300,
                dynamite,
                EventKind::Other(skills::EXPLOSION_EVENT),
            )
            .unwrap();
        queue.insert(now, 300, 0, EventKind::Other(7)).unwrap();
        queue.insert(now, 100, 0, EventKind::Other(7)).unwrap();
        let later = queue.insert(now, 9000, 0, EventKind::Other(7)).unwrap();
        let dues: Vec<u64> = queue.events().map(|event| event.due).collect();
        let ahead = dues.iter().filter(|due| **due <= now + 300).count();
        let lines = preview.timer_lines();
        assert_eq!(lines[1].1, ColorKey::RED, "{lines:?}");
        assert!(lines[1].0.contains("dynamite countdown"), "{lines:?}");
        assert!(lines[1].0.contains("30.0s"), "{lines:?}");
        assert!(lines[2..].iter().all(|line| line.1 == ColorKey::GREEN));
        // One press reaches it, past the event in the way and no further.
        press(&mut preview, "[Fast-Forward");
        let commands = preview.take_debug_commands();
        assert!(
            commands.len() >= 2 && commands.len() < ahead,
            "{commands:?}"
        );
        assert!(
            commands
                .iter()
                .all(|command| *command == DebugCommand::FastForwardTimer)
        );
        for command in &commands {
            preview.apply_debug(command).unwrap();
        }
        let world = &preview.host.world;
        assert_eq!(world.clock.ticks, now + 300);
        assert!(!world.queue.events().any(|event| event.due <= now + 300));
        assert!(world.queue.events().any(|event| event.id == later));
        // In combat and dialogue the queue stands still, so the panel refuses.
        let now = preview.host.world.clock.ticks;
        preview
            .host
            .world
            .queue
            .insert(now, 50, 0, EventKind::Other(skills::PREMATURE_EVENT))
            .unwrap();
        preview.host.world.combat = true;
        press(&mut preview, "[Fast-Forward");
        assert_eq!(
            note(&preview),
            "the queue does not run in combat or dialogue"
        );
        assert!(preview.take_debug_commands().is_empty());
    }

    #[test]
    fn a_live_page_redraws_when_the_clock_moves() {
        let dir = synthetic_corpus();
        let mut preview = campaign_world(dir.path());
        preview.debug_tick();
        assert!(!preview.debug_tick(), "the watch view waits for a change");
        press(&mut preview, "Timers");
        preview.debug_tick();
        assert!(!preview.debug_tick(), "nothing moved");
        preview.host.world.clock.advance(1).unwrap();
        assert!(preview.debug_tick());
    }

    #[test]
    fn the_world_tab_warps_and_spawns() {
        let dir = synthetic_corpus();
        let root = dir.path();
        campaign::write_campaign(root, Edition::Fallout1);
        // A key among the items.
        let mut key = fixture::prototype(0, 6);
        fixture::word(&mut key, 0, 60);
        std::fs::write(root.join("proto/items/key.pro"), key).unwrap();
        let list = root.join("proto/items/items.lst");
        let rows = std::fs::read_to_string(&list).unwrap();
        let rows: Vec<&str> = rows.lines().collect();
        let rows: Vec<&str> = (0..rows.len())
            .map(|row| if row == 59 { "key.pro" } else { rows[row] })
            .collect();
        std::fs::write(&list, rows.join("\n") + "\n").unwrap();
        let mut preview = open(root);
        press(&mut preview, "World");
        // The campaign's maps, and no number the game cannot load.
        assert_eq!(preview.warp_maps(), [10, 11, 12]);
        press(&mut preview, "  11");
        assert_eq!(preview.debug.as_ref().unwrap().cursor, 1);
        press(&mut preview, "Warp to Map");
        let warp = DebugCommand::TeleportMap {
            map_number: 11,
            elevation: 0,
            tile: None,
        };
        assert_eq!(preview.take_debug_commands(), std::slice::from_ref(&warp));
        // The keys do what they do on the map jump.
        preview.step(KeyCode::ArrowDown);
        preview.step(KeyCode::ArrowDown);
        preview.step(KeyCode::ArrowUp);
        preview.step(KeyCode::Enter);
        assert_eq!(preview.take_debug_commands(), [warp]);
        // C4 is Plastic Explosives (SPEC-013 §3.10), at the chosen quantity.
        press(&mut preview, "C4");
        press(&mut preview, "Qty: 1");
        press(&mut preview, "C4");
        let commands = preview.take_debug_commands();
        assert_eq!(
            commands,
            [
                DebugCommand::SpawnItem {
                    pid: skills::PLASTIC,
                    quantity: 1,
                },
                DebugCommand::SpawnItem {
                    pid: skills::PLASTIC,
                    quantity: 5,
                },
            ]
        );
        let player = preview.host.world.player;
        for command in &commands {
            preview.apply_debug(command).unwrap();
        }
        assert_eq!(
            preview.host.world.inventory_count(player, skills::PLASTIC),
            6
        );
        // Any item, by its typed PID.
        press(&mut preview, "Item");
        assert_eq!(note(&preview), "type a PID first");
        assert!(preview.take_debug_commands().is_empty());
        for key in [KeyCode::Digit5, KeyCode::Digit1] {
            preview.step(key);
        }
        press(&mut preview, "Item");
        assert_eq!(
            preview.take_debug_commands(),
            [DebugCommand::SpawnItem {
                pid: skills::DYNAMITE,
                quantity: 5,
            }]
        );
        // The game's own key.
        press(&mut preview, "Keycard");
        let commands = preview.take_debug_commands();
        assert_eq!(
            commands,
            [DebugCommand::SpawnItem {
                pid: 60,
                quantity: 5,
            }]
        );
        preview.apply_debug(&commands[0]).unwrap();
        assert_eq!(preview.host.world.inventory_count(player, 60), 5);
    }

    #[test]
    fn a_game_without_key_items_says_so() {
        let dir = synthetic_corpus();
        let mut preview = campaign_world(dir.path());
        press(&mut preview, "World");
        press(&mut preview, "Keycard");
        assert_eq!(note(&preview), "this game has no key items");
        assert!(preview.take_debug_commands().is_empty());
    }

    /// The campaign with the scripted door's `use_p_proc` opening with a
    /// critical section of three plain instructions, and that door's script
    /// loaded. Returns the first map with the overlay open on it.
    fn stepping_world(root: &std::path::Path) -> MapPreview {
        use state_fixture::{instruction, program, returning};
        campaign::write_campaign(root, Edition::Fallout1);
        let body = [0x8002, 0x8000, 0x8000, 0x8000, 0x8003, 0x8000].map(instruction);
        let script = program(
            &[],
            &[
                (b"start", returning(Vec::new())),
                (b"use_p_proc", returning(body.concat())),
            ],
        );
        std::fs::write(root.join("scripts/campdoor.int"), script).unwrap();
        let mut preview = open(root);
        preview
            .apply_debug(&DebugCommand::FireProc {
                object: campaign::SCRIPTED_DOOR,
                event: ScriptEvent::Use.code(),
                fixed: 0,
            })
            .unwrap();
        inspect(&mut preview, campaign::SCRIPTED_DOOR);
        preview
    }
    fn next_word(preview: &MapPreview) -> u16 {
        let vm = preview.host.debug_vm.as_ref().unwrap();
        decode_with(vm.code(), vm.pc, DecodeMode::of(&vm.program))
            .unwrap()
            .word
    }

    #[test]
    fn the_trace_tab_steps_a_procedure_and_filters_by_the_stack() {
        let dir = synthetic_corpus();
        let mut preview = stepping_world(dir.path());
        press(&mut preview, "Trace");
        press(&mut preview, "Step");
        assert_eq!(note(&preview), "press Begin first");
        press(&mut preview, "Abort");
        assert_eq!(note(&preview), "press Begin first");
        assert!(preview.take_debug_commands().is_empty());
        // Pause holds the world; the same button lets it go.
        press(&mut preview, "Pause");
        apply(&mut preview);
        assert!(preview.host.debug_paused);
        press(&mut preview, "Resume");
        apply(&mut preview);
        assert!(!preview.host.debug_paused);
        press(&mut preview, "Begin");
        let begin = preview.take_debug_commands();
        assert_eq!(
            begin,
            [DebugCommand::VmBegin {
                object: campaign::SCRIPTED_DOOR,
                event: ScriptEvent::Use.code(),
            }]
        );
        preview.apply_debug(&begin[0]).unwrap();
        press(&mut preview, "Begin");
        assert_eq!(note(&preview), "a procedure is being stepped");
        press(&mut preview, "Resume");
        assert_eq!(note(&preview), "the stepped procedure holds the world");
        assert!(preview.take_debug_commands().is_empty());
        let lines = preview.trace_lines_shown(preview.debug.as_ref().unwrap());
        assert!(lines[0].0.starts_with("stepping vm"), "{lines:?}");
        assert!(lines[1].0.starts_with("next: "), "{lines:?}");
        press(&mut preview, "Step");
        assert_eq!(
            preview.take_debug_commands(),
            [DebugCommand::VmStep { count: 1 }]
        );
        press(&mut preview, "Abort");
        assert_eq!(preview.take_debug_commands(), [DebugCommand::VmAbort]);
        // The filter hides what runs deeper than its limit.
        let overlay = preview.debug.as_ref().unwrap();
        let all = overlay.filtered_trace(TRACE_WINDOW);
        assert!(!all.is_empty());
        let depths = overlay.inspector.live_entries(TRACE_WINDOW);
        let deepest = depths.iter().map(|entry| entry.return_depth).max().unwrap();
        assert!(deepest > 0);
        press(&mut preview, "Stack: all");
        press(&mut preview, "+");
        assert_eq!(preview.debug.as_ref().unwrap().stack_limit, Some(1));
        press(&mut preview, "-");
        press(&mut preview, "-");
        assert_eq!(preview.debug.as_ref().unwrap().stack_limit, Some(0));
        let overlay = preview.debug.as_mut().unwrap();
        overlay.stack_limit = Some(deepest - 1);
        assert!(overlay.filtered_trace(TRACE_WINDOW).len() < all.len());
        press(&mut preview, "Stack <=");
        let overlay = preview.debug.as_ref().unwrap();
        assert_eq!(overlay.stack_limit, None);
        assert_eq!(overlay.filtered_trace(TRACE_WINDOW).len(), all.len());
    }

    #[test]
    fn step_into_critical_decides_how_far_a_step_goes() {
        let dir = synthetic_corpus();
        let mut preview = stepping_world(dir.path());
        preview
            .apply_debug(&DebugCommand::VmBegin {
                object: campaign::SCRIPTED_DOOR,
                event: ScriptEvent::Use.code(),
            })
            .unwrap();
        press(&mut preview, "Trace");
        while next_word(&preview) != 0x8002 {
            press(&mut preview, "Step");
            apply(&mut preview);
        }
        let critical = |preview: &MapPreview| {
            preview.host.debug_vm.as_ref().unwrap().status.0 & VM_CRITICAL != 0
        };
        // Off, the whole section is one Step, which ends just past it.
        press(&mut preview, "Step");
        assert_eq!(
            preview.take_debug_commands(),
            [DebugCommand::VmStep { count: 5 }]
        );
        // On, a Step stops on every instruction inside it.
        press(&mut preview, "Step Into Critical");
        assert!(widget(&preview, "Step Into Critical: on").on);
        for _ in 0..4 {
            press(&mut preview, "Step");
            let commands = preview.take_debug_commands();
            assert_eq!(commands, [DebugCommand::VmStep { count: 1 }]);
            preview.apply_debug(&commands[0]).unwrap();
            assert!(critical(&preview));
        }
        // Off again, from inside: the rest of the section.
        press(&mut preview, "Step Into Critical");
        assert_eq!(next_word(&preview), 0x8003);
        press(&mut preview, "Step");
        assert_eq!(
            preview.take_debug_commands(),
            [DebugCommand::VmStep { count: 1 }]
        );
        preview
            .apply_debug(&DebugCommand::VmStep { count: 1 })
            .unwrap();
        assert!(!critical(&preview));
        assert_eq!(next_word(&preview), 0x8000);
    }

    /// A program's initial stream opens with a critical section whose first
    /// jump goes over its tables (SPEC-004 §2.3): the count stops at the jump
    /// instead of reading the tables as code.
    #[test]
    fn a_step_stops_where_the_section_leaves_the_line() {
        let dir = synthetic_corpus();
        let mut preview = campaign_world(dir.path());
        let mut vm = preview.host.programs.values().next().unwrap().clone();
        vm.pc = 0;
        vm.status.0 = 0;
        preview.host.debug_vm = Some(vm);
        let vm = preview.host.debug_vm.as_ref().unwrap();
        let mode = DecodeMode::of(&vm.program);
        let count = preview.step_count(preview.debug.as_ref().unwrap());
        let mut at = 0;
        let mut last = 0;
        for _ in 0..count {
            let instruction = decode_with(vm.code(), at, mode).unwrap();
            last = instruction.word;
            at = instruction.next_address();
        }
        assert!(count > 1);
        assert!(leaves_the_line(last), "{last:#x}");
    }

    #[test]
    fn the_bot_tab_runs_watch_mode() {
        let dir = synthetic_corpus();
        let mut preview = campaign_world(dir.path());
        press(&mut preview, "Bot");
        for label in ["Pause", "Resume", "Stop", "1x", "Max"] {
            press(&mut preview, label);
            assert_eq!(note(&preview), "Watch Mode is off");
        }
        assert!(preview.watch.is_none());
        press(&mut preview, "Start");
        let watch = preview.watch.as_ref().unwrap();
        assert!(!watch.paused);
        assert_eq!(watch.speed, BotSpeed::X1);
        preview.image().unwrap();
        // Both reach the state they name, whatever the press did on the way.
        press(&mut preview, "Pause");
        assert!(preview.watch.as_ref().unwrap().paused);
        press(&mut preview, "Pause");
        assert!(preview.watch.as_ref().unwrap().paused);
        press(&mut preview, "Resume");
        assert!(!preview.watch.as_ref().unwrap().paused);
        for speed in BotSpeed::ALL {
            press(&mut preview, speed.label());
            assert_eq!(preview.watch.as_ref().unwrap().speed, speed);
            assert!(widget(&preview, speed.label()).on);
        }
        press(&mut preview, "Stop");
        assert!(preview.watch.is_none());
        assert!(preview.take_debug_commands().is_empty(), "no world change");
    }

    /// On the unlocked world `click` gets a canvas pixel. One with the panel's
    /// numbers is a map pick, not a press on the button drawn at those
    /// numbers; the panel itself still takes presses at frame pixels.
    #[test]
    fn the_unlocked_world_never_presses_the_panel_by_its_numbers() {
        let dir = synthetic_corpus();
        let mut preview = interactive(dir.path());
        preview.set_enhanced(Some(EnhancedSettings {
            smooth_camera: false,
            ..Default::default()
        }));
        preview.tick(16).unwrap();
        preview.set_debug(true);
        press(&mut preview, "Bot");
        let layout = EnhancedLayout::compute([1920, 1080], None, preview.enhanced_zoom()).unwrap();
        preview.enhanced_frame(&layout).unwrap().unwrap();
        let at = |point: [f64; 2]| PointerAt {
            ui: layout.ui_pixel(point),
            world: layout.world_point(point),
            side: layout.in_side_panel(point),
        };
        // The canvas pixel of the Start button's middle.
        let [x, y, width, height] = widget(&preview, "Start").rect;
        let per_world = f64::from(layout.ui_scale * layout.zoom);
        let canvas = [x + width / 2, y + height / 2];
        let surface = [0, 1]
            .map(|axis| f64::from(layout.world_rect[axis]) + f64::from(canvas[axis]) * per_world);
        let pointer = at([surface[0] + 0.5, surface[1] + 0.5]);
        assert!(
            !pointer.ui.is_some_and(|ui| inside(PANEL, ui)),
            "not at the panel on the frame"
        );
        assert!(pointer.world.is_some());
        assert!(preview.enhanced_button(pointer, PointerButton::Left, true));
        preview.enhanced_button(pointer, PointerButton::Left, false);
        assert!(preview.watch.is_none(), "the button was not pressed");
        assert_eq!(page(&preview), Page::Inspect, "the map took the click");
        // The same tab at its frame pixel.
        let [x, y, width, height] = widget(&preview, "Bot").rect;
        let ui = [x + width / 2, y + height / 2];
        let surface = [0, 1].map(|axis| {
            f64::from(layout.ui_rect[axis])
                + (f64::from(ui[axis]) + 0.5) * f64::from(layout.ui_scale)
        });
        let pointer = at(surface);
        assert_eq!(pointer.ui, Some(ui));
        assert!(preview.enhanced_button(pointer, PointerButton::Left, true));
        assert_eq!(page(&preview), Page::Bot);
    }

    /// A tab draws on its panel and nowhere else.
    #[test]
    fn every_tab_draws_over_the_map_alone() {
        let dir = synthetic_corpus();
        let mut preview = campaign_world(dir.path());
        let shown = |image: &vault_render::IndexedImage| -> Vec<u8> {
            let stride = image.width() as usize;
            (0..PANEL[3] as usize)
                .flat_map(|y| image.pixels()[y * stride..y * stride + PANEL[0] as usize].to_vec())
                .collect()
        };
        let before = preview.image().unwrap();
        inspect(&mut preview, campaign::FRIEND);
        for (label, _) in TABS {
            press(&mut preview, label);
            let image = preview.image().unwrap();
            assert!(shown(&image) == shown(&before), "{label} left the panel");
        }
    }
}
