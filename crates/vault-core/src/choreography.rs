//! SPEC-007 §3.10 animation sequences. A sequence is an ordered list of
//! entries, each with an owner and a start delay, run by the §3.10.1 start
//! rule ([`Schedule`]). Instant entries finish as they start; timed entries
//! take frame steps at their own interval. A [`Driver`] says what entries
//! do: the world's own sequences (SPEC-007 §3.11) act on world objects.
//!
//! An attack's [`Sequence`] is presentation. The world applies the attack
//! at once, and the sequence only shows it: each object it owns is drawn
//! from the sequence's display state for that object until it ends.
use crate::{
    ObjectRef,
    ai::TauntCallback,
    motion::{HEX_STEPS, crosses},
    routing::step,
};
use std::collections::BTreeMap;

/// The most entries one sequence holds (§3.10.1).
pub const CAPACITY: usize = 40;

/// What an entry shows: a world object, or an object that exists only in
/// the presentation (a non-thrown weapon's projectile, explosion art).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Owner {
    World(ObjectRef),
    Shown(u32),
}

/// How a sequence draws one owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    pub pid: u32,
    pub art: u32,
    pub frame: usize,
    /// `None` while the object is off the map.
    pub hex: Option<i32>,
    pub elevation: u8,
    pub orientation: u8,
    pub displacement: [i32; 2],
    pub hidden: bool,
    pub flat: bool,
    /// SPEC-014 §3.44 the display's latest reinsertion into its hex list
    /// (a [`crate::world::next_insertion`] stamp), or `None` while it keeps
    /// the place its world object has.
    pub inserted: Option<u64>,
}

impl Display {
    /// SPEC-014 §3.44 the display is moved to the end of its group on its
    /// hex: a hex move, a pixel-offset change or a frame step.
    fn reinsert(&mut self) {
        self.inserted = Some(crate::world::next_insertion());
    }
}

/// A timed entry's frames: each frame's displacement in the owner's
/// direction (SPEC-001 §3.5 increments), and the frame-step interval.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Frames {
    pub shifts: Vec<[i32; 2]>,
    pub interval_ms: u64,
}

/// What an entry does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Face `orientation` in the standing pose `art` (instant).
    Turn {
        orientation: u8,
        art: u32,
    },
    /// A sound effect (instant); an empty name is a silent entry.
    Sound {
        name: Vec<u8>,
        object: Option<ObjectRef>,
    },
    /// A taunt's display, by index into the show's taunts (instant).
    Taunt(usize),
    /// Release the show's extra sequence of this index (instant): the
    /// placeholder of §3.10.6.
    Release(usize),
    Hide,
    Unhide,
    /// New art at frame 0 (instant).
    SetArt {
        art: u32,
    },
    /// Onto the map at a hex (instant): a dropped object.
    Place {
        hex: i32,
        elevation: u8,
    },
    /// Off the map (instant): an erased or destroyed object.
    Remove,
    /// The corpse presentation's callback (instant): the corpse lies flat
    /// when `flat`, and the items it drops lie on its hex.
    Corpse {
        flat: bool,
        dropped: Vec<ObjectRef>,
        hex: i32,
        elevation: u8,
    },
    /// Play `art` once, one frame per step, then finish (and hide when
    /// `hide`) on the step after the last frame (timed).
    Animate {
        art: u32,
        frames: Frames,
        hide: bool,
    },
    /// Move node by node, one node per step, showing `art` with its frames
    /// advancing; the step that reaches the last node finishes (timed).
    Move {
        art: u32,
        frames: Frames,
        nodes: Vec<(i32, [i32; 2])>,
    },
    /// Walk a route the world has already taken, from the owner's shown
    /// hex, by the SPEC-007 §3.11.1 step model, with `arts` and `frames`
    /// for each facing (timed).
    Walk {
        arts: [u32; 6],
        frames: Box<[Frames; 6]>,
        route: Vec<u8>,
        cursor: usize,
        frame: usize,
    },
}
impl Action {
    fn timed(&self) -> bool {
        matches!(
            self,
            Self::Animate { .. } | Self::Move { .. } | Self::Walk { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Waiting,
    /// Frame steps taken, and when the next one is due.
    Running {
        steps: usize,
        due_ms: u64,
    },
    Finished,
}

/// One entry of a sequence: its owner, start delay and action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<O, A> {
    pub owner: O,
    pub delay: i32,
    pub action: A,
    /// A forced callback or erase: it still runs when the sequence stops
    /// early (§3.10.1).
    pub forced: bool,
    state: State,
}

/// How an entry starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Begin {
    /// An instant entry took effect and finished.
    Done,
    /// A timed entry runs; its first frame step is due at once.
    Timed,
    /// The entry failed as it started: the sequence stops (§3.10.1).
    Failed,
}
/// What a running entry's frame step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stepped {
    /// It runs on; the next step is due this many milliseconds later.
    After(u64),
    /// It finished.
    Done,
}

/// What a sequence's entries do when they start and step: the start rule
/// itself is the [`Schedule`]'s.
pub trait Driver<O, A> {
    fn start(&mut self, entry: &mut Entry<O, A>, now_ms: u64) -> Begin;
    /// Frame step `step` (1 for the first) of a running entry.
    fn step(&mut self, entry: &mut Entry<O, A>, step: usize, now_ms: u64) -> Stepped;
    /// Entries (owner, delay, action) that the entry which just started
    /// registered as it ran: they follow it in its sequence, so a delay 0
    /// starts them in the same examination (SPEC-007 §3.11.4: a container
    /// use registers the container's sound and animation from inside the
    /// user's sequence, and the entries after it wait for them).
    fn appended(&mut self) -> Vec<(O, i32, A)> {
        Vec::new()
    }
    /// Whether a running entry never finishes on its own (a repeating
    /// script animation): instant playback stops once only such entries
    /// run, instead of stepping them to its pass limit.
    fn endless(&self, _entry: &Entry<O, A>) -> bool {
        false
    }
}

/// What an examination or a pass started that the host acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    Sound {
        name: Vec<u8>,
        object: Option<ObjectRef>,
    },
    Taunt(usize),
    Release(usize),
}

/// The registration failure of §3.10.1: the sequence is discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// More than [`CAPACITY`] entries.
    Full,
    /// An animate, set-art or take-out entry's art cannot be loaded.
    Art(u32),
}

/// The §3.10.1 start rule over one sequence's entries, whatever they do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule<O, A> {
    entries: Vec<Entry<O, A>>,
    /// The next entry to start; every earlier one has started.
    next: usize,
    registered: bool,
    /// An entry failed as it started, or the sequence was cancelled.
    stopped: bool,
}
impl<O, A> Default for Schedule<O, A> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next: 0,
            registered: false,
            stopped: false,
        }
    }
}
impl<O, A> Schedule<O, A> {
    /// Append an entry; past [`CAPACITY`] the sequence cannot register.
    pub fn push(&mut self, owner: O, delay: i32, action: A, forced: bool) -> Result<(), Refused> {
        if self.entries.len() >= CAPACITY {
            return Err(Refused::Full);
        }
        self.entries.push(Entry {
            owner,
            delay,
            action,
            forced,
            state: State::Waiting,
        });
        Ok(())
    }
    pub fn entries(&self) -> &[Entry<O, A>] {
        &self.entries
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn registered(&self) -> bool {
        self.registered
    }
    /// An entry failed as it started, or the sequence was cancelled.
    pub fn stopped(&self) -> bool {
        self.stopped
    }
    /// Every entry has finished.
    pub fn finished(&self) -> bool {
        self.entries
            .iter()
            .all(|entry| entry.state == State::Finished)
    }
    /// Whether entry `index` has started (running or finished).
    pub fn started(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|entry| entry.state != State::Waiting)
    }
    /// Whether entry `index` is running.
    pub fn running(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|entry| matches!(entry.state, State::Running { .. }))
    }
    /// Registration at `now_ms`: the first entry's delay is forced to 0,
    /// then the sequence is examined once.
    pub fn register(&mut self, now_ms: u64, driver: &mut impl Driver<O, A>) {
        self.registered = true;
        if let Some(first) = self.entries.first_mut() {
            first.delay = 0;
        }
        self.examine(now_ms, driver);
    }
    /// §3.10.1 an examination: start the next entry whatever its delay
    /// when every started entry has finished; otherwise a negative delay
    /// waits, a positive one counts down by one, and 0 starts it.
    fn examine(&mut self, now_ms: u64, driver: &mut impl Driver<O, A>) {
        while !self.stopped && self.next < self.entries.len() {
            let settled = self.entries[..self.next]
                .iter()
                .all(|entry| entry.state == State::Finished);
            if !settled {
                let entry = &mut self.entries[self.next];
                if entry.delay < 0 {
                    return;
                }
                if entry.delay > 0 {
                    entry.delay -= 1;
                    return;
                }
            }
            let index = self.next;
            self.next += 1;
            self.start(index, now_ms, driver);
        }
    }
    /// Start an entry: an instant one takes effect and finishes; a timed
    /// one takes its first frame step at the next pass that reaches it.
    /// Entries the driver registered meanwhile are placed right after it,
    /// as the next to start; past [`CAPACITY`] they are dropped (safe
    /// bound: the reference's full-list registration failure).
    fn start(&mut self, index: usize, now_ms: u64, driver: &mut impl Driver<O, A>) {
        let begin = driver.start(&mut self.entries[index], now_ms);
        let entry = &mut self.entries[index];
        entry.state = match begin {
            Begin::Timed => State::Running {
                steps: 0,
                due_ms: now_ms,
            },
            Begin::Done | Begin::Failed => State::Finished,
        };
        for (offset, (owner, delay, action)) in driver.appended().into_iter().enumerate() {
            if self.entries.len() >= CAPACITY {
                break;
            }
            self.entries.insert(
                self.next + offset,
                Entry {
                    owner,
                    delay,
                    action,
                    forced: false,
                    state: State::Waiting,
                },
            );
        }
        if begin == Begin::Failed {
            self.stop(now_ms, driver);
        }
    }
    /// §3.10.1 a sequence that stops while playing: running entries end,
    /// unstarted forced entries run at once, and every other unstarted
    /// entry (sounds among them) is dropped.
    fn stop(&mut self, now_ms: u64, driver: &mut impl Driver<O, A>) {
        self.stopped = true;
        for index in 0..self.entries.len() {
            let entry = &mut self.entries[index];
            if entry.state == State::Waiting && entry.forced {
                driver.start(entry, now_ms);
            }
            entry.state = State::Finished;
        }
        self.next = self.entries.len();
    }
    /// Stop the sequence as an entry failing would (§3.10.1): running
    /// entries end, unstarted forced entries run at once, and the rest are
    /// dropped.
    pub fn halt(&mut self, now_ms: u64, driver: &mut impl Driver<O, A>) {
        if !self.stopped {
            self.stop(now_ms, driver);
        }
    }
    /// End the sequence where it stands, running nothing more.
    pub fn cancel(&mut self) {
        self.stopped = true;
        for entry in &mut self.entries {
            entry.state = State::Finished;
        }
        self.next = self.entries.len();
    }
    /// One scheduler pass at `now_ms`: each running timed entry whose step
    /// is due takes one frame step, earlier-started entries first; an entry
    /// started during the pass takes its first step in it.
    pub fn pass(&mut self, now_ms: u64, driver: &mut impl Driver<O, A>) {
        if !self.registered {
            return;
        }
        let mut index = 0;
        while index < self.entries.len() && !self.stopped {
            if matches!(self.entries[index].state, State::Running { due_ms, .. } if due_ms <= now_ms)
            {
                self.step(index, now_ms, driver);
            }
            index += 1;
        }
    }
    /// SPEC-014 §3.28 background processing paused for `span_ms`: every
    /// running entry's next step moves by the span, so the sequence goes on
    /// where it stood instead of replaying the paused time at once.
    pub fn delay(&mut self, span_ms: u64) {
        for entry in &mut self.entries {
            if let State::Running { due_ms, .. } = &mut entry.state {
                *due_ms = due_ms.saturating_add(span_ms);
            }
        }
    }
    /// The earliest time a running entry's step is due.
    pub fn due_ms(&self) -> Option<u64> {
        self.entries
            .iter()
            .filter_map(|entry| match entry.state {
                State::Running { due_ms, .. } => Some(due_ms),
                _ => None,
            })
            .min()
    }
    /// One frame step of a running entry: the sequence is examined at its
    /// start, and again when the step finishes the entry.
    fn step(&mut self, index: usize, now_ms: u64, driver: &mut impl Driver<O, A>) {
        self.examine(now_ms, driver);
        let State::Running { steps, .. } = self.entries[index].state else {
            return;
        };
        let step = steps + 1;
        let stepped = driver.step(&mut self.entries[index], step, now_ms);
        let entry = &mut self.entries[index];
        match (stepped, &mut entry.state) {
            (Stepped::After(interval), State::Running { steps, due_ms }) => {
                *steps = step;
                *due_ms += interval.max(1);
            }
            (Stepped::Done, _) => {
                entry.state = State::Finished;
                self.examine(now_ms, driver);
            }
            // The step stopped the sequence.
            _ => {}
        }
    }
    /// Run every remaining step at once. Returns the time of the last pass.
    /// A sequence whose only running entries never finish (a repeating
    /// animation, [`Driver::endless`]) is left running after their first
    /// step rather than stepped to the pass limit.
    pub fn run_to_end(&mut self, mut now_ms: u64, driver: &mut impl Driver<O, A>) -> u64 {
        if !self.registered {
            self.register(now_ms, driver);
        }
        for _ in 0..100_000 {
            let Some(due) = self.due_ms() else {
                break;
            };
            now_ms = now_ms.max(due);
            self.pass(now_ms, driver);
            let settled = self
                .entries
                .iter()
                .filter(|entry| matches!(entry.state, State::Running { .. }))
                .all(|entry| driver.endless(entry));
            if settled {
                break;
            }
        }
        now_ms
    }
}

/// The presentation's driver: entries act on the sequence's displays and
/// report what the host plays.
struct Present<'a> {
    displays: &'a mut BTreeMap<Owner, Display>,
    started: &'a mut Vec<Started>,
}
impl Driver<Owner, Action> for Present<'_> {
    fn start(&mut self, entry: &mut Entry<Owner, Action>, _now_ms: u64) -> Begin {
        let owner = entry.owner;
        if entry.action.timed() {
            if let Some(display) = self.displays.get_mut(&owner) {
                display.displacement = [0; 2];
                display.reinsert();
            }
            return Begin::Timed;
        }
        match &entry.action {
            Action::Sound { name, object } => self.started.push(Started::Sound {
                name: name.clone(),
                object: *object,
            }),
            Action::Taunt(index) => self.started.push(Started::Taunt(*index)),
            Action::Release(index) => self.started.push(Started::Release(*index)),
            Action::Corpse {
                flat,
                dropped,
                hex,
                elevation,
            } => {
                if *flat && let Some(display) = self.displays.get_mut(&owner) {
                    display.flat = true;
                    display.reinsert();
                }
                for item in dropped {
                    if let Some(display) = self.displays.get_mut(&Owner::World(*item)) {
                        display.hex = Some(*hex);
                        display.elevation = *elevation;
                        display.hidden = false;
                        display.reinsert();
                    }
                }
            }
            action => {
                if let Some(display) = self.displays.get_mut(&owner) {
                    apply(display, action);
                }
            }
        }
        Begin::Done
    }
    /// Step k shows frame k - 1 (an animation) or node k - 1 (a move). The
    /// step after an animation's last frame, or the one that reaches a
    /// move's last node, finishes the entry.
    fn step(&mut self, entry: &mut Entry<Owner, Action>, step: usize, _now_ms: u64) -> Stepped {
        let display = self.displays.get_mut(&entry.owner);
        if let Action::Walk {
            arts,
            frames,
            route,
            cursor,
            frame,
        } = &mut entry.action
        {
            return match display {
                Some(display) => walk(display, arts, frames, route, cursor, frame, step),
                None => Stepped::Done,
            };
        }
        match &entry.action {
            Action::Animate { art, frames, hide } => {
                if let Some(shift) = frames.shifts.get(step - 1) {
                    if let Some(display) = display {
                        display.art = *art;
                        display.frame = step - 1;
                        display.hidden = false;
                        display.displacement[0] += shift[0];
                        display.displacement[1] += shift[1];
                        display.reinsert();
                    }
                    Stepped::After(frames.interval_ms)
                } else {
                    if let Some(display) = display {
                        // Art that could not be loaded still takes effect.
                        if frames.shifts.is_empty() {
                            display.art = *art;
                            display.frame = 0;
                        }
                        if *hide {
                            display.hidden = true;
                        }
                    }
                    Stepped::Done
                }
            }
            Action::Move { art, frames, nodes } => {
                if let (Some(display), Some((hex, offset))) = (display, nodes.get(step - 1)) {
                    display.art = *art;
                    display.frame = (step - 1) % frames.shifts.len().max(1);
                    display.hex = Some(*hex);
                    display.displacement = *offset;
                    display.reinsert();
                }
                if step >= nodes.len() {
                    Stepped::Done
                } else {
                    Stepped::After(frames.interval_ms)
                }
            }
            _ => Stepped::Done,
        }
    }
}

/// One reserved, prioritized attack sequence: the start rule over
/// presentation entries, and the displays they act on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sequence {
    schedule: Schedule<Owner, Action>,
    displays: BTreeMap<Owner, Display>,
}
impl Sequence {
    /// An owner's display before the sequence plays.
    pub fn show(&mut self, owner: Owner, mut display: Display) {
        // SPEC-014 §3.44: an object only the presentation knows is created
        // now, at the end of its group.
        if matches!(owner, Owner::Shown(_)) && display.inserted.is_none() {
            display.reinsert();
        }
        self.displays.entry(owner).or_insert(display);
    }
    /// Append an entry; past [`CAPACITY`] the sequence cannot register.
    pub fn push(&mut self, owner: Owner, delay: i32, action: Action) -> Result<(), Refused> {
        self.schedule.push(owner, delay, action, false)
    }
    /// Append a forced callback or erase.
    pub fn push_forced(&mut self, owner: Owner, delay: i32, action: Action) -> Result<(), Refused> {
        self.schedule.push(owner, delay, action, true)
    }
    pub fn entries(&self) -> &[Entry<Owner, Action>] {
        self.schedule.entries()
    }
    pub fn len(&self) -> usize {
        self.schedule.len()
    }
    pub fn is_empty(&self) -> bool {
        self.schedule.is_empty()
    }
    pub fn registered(&self) -> bool {
        self.schedule.registered()
    }
    /// Every entry has finished.
    pub fn finished(&self) -> bool {
        self.schedule.finished()
    }
    /// Whether entry `index` has started (running or finished).
    pub fn started(&self, index: usize) -> bool {
        self.schedule.started(index)
    }
    /// Whether entry `index` is running.
    pub fn running(&self, index: usize) -> bool {
        self.schedule.running(index)
    }
    pub fn display(&self, owner: Owner) -> Option<&Display> {
        self.displays.get(&owner)
    }
    pub fn displays(&self) -> impl Iterator<Item = (&Owner, &Display)> {
        self.displays.iter()
    }
    /// Registration at `now_ms`; returns what started.
    pub fn register(&mut self, now_ms: u64) -> Vec<Started> {
        let mut started = Vec::new();
        self.schedule.register(
            now_ms,
            &mut Present {
                displays: &mut self.displays,
                started: &mut started,
            },
        );
        started
    }
    /// One scheduler pass at `now_ms`; returns what started.
    pub fn pass(&mut self, now_ms: u64) -> Vec<Started> {
        let mut started = Vec::new();
        self.schedule.pass(
            now_ms,
            &mut Present {
                displays: &mut self.displays,
                started: &mut started,
            },
        );
        started
    }
    /// The earliest time a running entry's step is due.
    pub fn due_ms(&self) -> Option<u64> {
        self.schedule.due_ms()
    }
    /// Move every running entry's next step by a paused span.
    pub fn delay(&mut self, span_ms: u64) {
        self.schedule.delay(span_ms);
    }
    /// Run every remaining step at once (headless playback, and the final
    /// state an attack leaves). Returns what started, in order.
    pub fn run_to_end(&mut self, now_ms: u64) -> Vec<Started> {
        let mut started = Vec::new();
        self.schedule.run_to_end(
            now_ms,
            &mut Present {
                displays: &mut self.displays,
                started: &mut started,
            },
        );
        started
    }
}

/// One frame step of a shown walk (SPEC-007 §3.11.1): the first step
/// starts at frame 0 with no displacement, later ones advance a frame,
/// wrapping; the shown frame's displacement for the facing accumulates
/// until it reaches the next hex's pixel step, and then the walker enters
/// that hex with the residual. A facing whose frames never move enters a
/// hex every step. The step entering the last hex finishes, dropping the
/// residual.
fn walk(
    display: &mut Display,
    arts: &[u32; 6],
    frames: &[Frames; 6],
    route: &[u8],
    cursor: &mut usize,
    frame: &mut usize,
    step_number: usize,
) -> Stepped {
    // SPEC-014 §3.44: every walk step is an offset change, which moves
    // the walker to the end of its group on its hex.
    display.reinsert();
    let Some(&facing) = route.get(*cursor) else {
        display.displacement = [0; 2];
        return Stepped::Done;
    };
    let index = usize::from(facing % 6);
    let set = &frames[index];
    if step_number == 1 {
        *frame = 0;
        display.displacement = [0; 2];
    } else {
        *frame = (*frame + 1) % set.shifts.len().max(1);
    }
    display.art = arts[index];
    display.frame = *frame;
    display.orientation = facing;
    display.hidden = false;
    let moving = set.shifts.iter().any(|shift| *shift != [0; 2]);
    let shift = set.shifts.get(*frame).copied().unwrap_or([0; 2]);
    let moved = [
        display.displacement[0] + shift[0],
        display.displacement[1] + shift[1],
    ];
    if moving && !crosses(moved, facing) {
        display.displacement = moved;
        return Stepped::After(set.interval_ms);
    }
    let pixel = HEX_STEPS[index];
    display.hex = display.hex.map(|hex| step(hex, facing));
    *cursor += 1;
    let Some(&next) = route.get(*cursor) else {
        display.displacement = [0; 2];
        return Stepped::Done;
    };
    display.orientation = next;
    display.art = arts[usize::from(next % 6)];
    display.displacement = if moving {
        [moved[0] - pixel[0], moved[1] - pixel[1]]
    } else {
        [0; 2]
    };
    Stepped::After(set.interval_ms)
}

/// An instant action's effect on its owner's display.
fn apply(display: &mut Display, action: &Action) {
    match action {
        Action::Turn { orientation, art } => {
            display.orientation = *orientation;
            display.art = *art;
            display.frame = 0;
            display.displacement = [0; 2];
            display.reinsert();
        }
        Action::Hide => display.hidden = true,
        Action::Unhide => display.hidden = false,
        Action::SetArt { art } => {
            display.art = *art;
            display.frame = 0;
        }
        Action::Place { hex, elevation } => {
            display.hex = Some(*hex);
            display.elevation = *elevation;
            display.hidden = false;
            display.reinsert();
        }
        Action::Remove => display.hex = None,
        _ => {}
    }
}

/// One attack's presentation: the main sequence, the extra victims'
/// sequences it releases at its placeholders (§3.10.7), and the taunts its
/// entries display.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Show {
    pub main: Sequence,
    pub extras: Vec<Sequence>,
    pub taunts: Vec<TauntCallback>,
    /// It shows an explosion (SPEC-013 §3.10 in progress).
    pub explosive: bool,
    /// Queued while combat was active: it counts toward the SPEC-014 §3.27
    /// combat lock until it has played (`clicks::CombatLock`).
    pub combat: bool,
    /// SPEC-014 §3.44 a player attack under `target_highlight` 2: the
    /// target outlines, disabled as it started, are enabled again once it
    /// has played (its damage applied).
    pub outlines: bool,
}
impl Show {
    pub fn finished(&self) -> bool {
        self.main.finished() && self.extras.iter().all(Sequence::finished)
    }
    /// The display an owner has in this show.
    pub fn display(&self, owner: Owner) -> Option<&Display> {
        self.main
            .display(owner)
            .or_else(|| self.extras.iter().find_map(|extra| extra.display(owner)))
    }
    /// Every display of the show, main sequence first.
    pub fn displays(&self) -> impl Iterator<Item = (&Owner, &Display)> {
        self.main
            .displays()
            .chain(self.extras.iter().flat_map(Sequence::displays))
    }
    /// The main sequence's registration, the first time, and otherwise one
    /// scheduler pass over it and the released extras. A placeholder
    /// releases its extra at once: the extra registers then and, in a pass,
    /// takes its first steps in it. Returns the sounds and taunts started,
    /// in order.
    pub fn pass(&mut self, now_ms: u64) -> Vec<Started> {
        let passing = self.main.registered();
        let main = if passing {
            self.main.pass(now_ms)
        } else {
            self.main.register(now_ms)
        };
        let mut started = Vec::new();
        for item in main {
            match item {
                Started::Release(index) => {
                    if let Some(extra) = self.extras.get_mut(index)
                        && !extra.registered()
                    {
                        started.extend(extra.register(now_ms));
                    }
                }
                other => started.push(other),
            }
        }
        if passing {
            for extra in self.extras.iter_mut().filter(|extra| extra.registered()) {
                started.extend(extra.pass(now_ms));
            }
        }
        started
    }
    /// Move every running entry's next step by a paused span.
    pub fn delay(&mut self, span_ms: u64) {
        self.main.delay(span_ms);
        for extra in &mut self.extras {
            extra.delay(span_ms);
        }
    }
    /// The earliest due step of any running entry.
    pub fn due_ms(&self) -> Option<u64> {
        std::iter::once(self.main.due_ms())
            .chain(self.extras.iter().map(Sequence::due_ms))
            .flatten()
            .min()
    }
    /// Play the whole show at once; returns the sounds and taunts started.
    pub fn run_to_end(&mut self, mut now_ms: u64) -> Vec<Started> {
        let mut started = self.pass(now_ms);
        for _ in 0..100_000 {
            if self.finished() {
                break;
            }
            let Some(due) = self.due_ms() else {
                break;
            };
            now_ms = now_ms.max(due);
            started.extend(self.pass(now_ms));
        }
        started
    }
}
