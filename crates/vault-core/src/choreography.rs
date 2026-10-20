//! SPEC-007 §3.10 attack choreography. An attack registers one reserved
//! sequence: an ordered list of entries, each with an owner and a start
//! delay, run by the §3.10.1 start rule. Instant entries finish as they
//! start; timed entries take frame steps at their own interval.
//!
//! A sequence is presentation. The world applies the attack at once, and
//! the sequence only shows it: each object it owns is drawn from the
//! sequence's display state for that object until the sequence ends.
use crate::{ObjectRef, ai::TauntCallback};
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
}
impl Action {
    fn timed(&self) -> bool {
        matches!(self, Self::Animate { .. } | Self::Move { .. })
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

/// One entry of a sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub owner: Owner,
    pub delay: i32,
    pub action: Action,
    /// A forced callback or erase: it still runs when the sequence stops
    /// early (§3.10.1).
    pub forced: bool,
    state: State,
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

/// One reserved, prioritized animation sequence.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sequence {
    entries: Vec<Entry>,
    /// The next entry to start; every earlier one has started.
    next: usize,
    displays: BTreeMap<Owner, Display>,
    registered: bool,
}
impl Sequence {
    /// An owner's display before the sequence plays.
    pub fn show(&mut self, owner: Owner, display: Display) {
        self.displays.entry(owner).or_insert(display);
    }
    /// Append an entry; past [`CAPACITY`] the sequence cannot register.
    pub fn push(&mut self, owner: Owner, delay: i32, action: Action) -> Result<(), Refused> {
        self.push_entry(owner, delay, action, false)
    }
    /// Append a forced callback or erase.
    pub fn push_forced(&mut self, owner: Owner, delay: i32, action: Action) -> Result<(), Refused> {
        self.push_entry(owner, delay, action, true)
    }
    fn push_entry(
        &mut self,
        owner: Owner,
        delay: i32,
        action: Action,
        forced: bool,
    ) -> Result<(), Refused> {
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
    pub fn entries(&self) -> &[Entry] {
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
    pub fn display(&self, owner: Owner) -> Option<&Display> {
        self.displays.get(&owner)
    }
    pub fn displays(&self) -> impl Iterator<Item = (&Owner, &Display)> {
        self.displays.iter()
    }
    /// Registration at `now_ms`: the first entry's delay is forced to 0,
    /// then the sequence is examined once.
    pub fn register(&mut self, now_ms: u64) -> Vec<Started> {
        self.registered = true;
        if let Some(first) = self.entries.first_mut() {
            first.delay = 0;
        }
        let mut started = Vec::new();
        self.examine(now_ms, &mut started);
        started
    }
    /// §3.10.1 an examination: start the next entry whatever its delay
    /// when every started entry has finished; otherwise a negative delay
    /// waits, a positive one counts down by one, and 0 starts it.
    fn examine(&mut self, now_ms: u64, started: &mut Vec<Started>) {
        while self.next < self.entries.len() {
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
            self.start(index, now_ms, started);
        }
    }
    /// Start an entry: an instant one takes effect and finishes; a timed
    /// one takes its first frame step at the next pass that reaches it.
    fn start(&mut self, index: usize, now_ms: u64, started: &mut Vec<Started>) {
        let entry = &mut self.entries[index];
        let owner = entry.owner;
        if entry.action.timed() {
            entry.state = State::Running {
                steps: 0,
                due_ms: now_ms,
            };
            if let Some(display) = self.displays.get_mut(&owner) {
                display.displacement = [0; 2];
            }
            return;
        }
        entry.state = State::Finished;
        match &entry.action {
            Action::Sound { name, object } => started.push(Started::Sound {
                name: name.clone(),
                object: *object,
            }),
            Action::Taunt(index) => started.push(Started::Taunt(*index)),
            Action::Release(index) => started.push(Started::Release(*index)),
            Action::Corpse {
                flat,
                dropped,
                hex,
                elevation,
            } => {
                if *flat && let Some(display) = self.displays.get_mut(&owner) {
                    display.flat = true;
                }
                for item in dropped {
                    if let Some(display) = self.displays.get_mut(&Owner::World(*item)) {
                        display.hex = Some(*hex);
                        display.elevation = *elevation;
                        display.hidden = false;
                    }
                }
            }
            action => {
                if let Some(display) = self.displays.get_mut(&owner) {
                    apply(display, action);
                }
            }
        }
    }
    /// One scheduler pass at `now_ms`: each running timed entry whose step
    /// is due takes one frame step, earlier-started entries first; an entry
    /// started during the pass takes its first step in it.
    pub fn pass(&mut self, now_ms: u64) -> Vec<Started> {
        let mut started = Vec::new();
        if !self.registered {
            return started;
        }
        let mut index = 0;
        while index < self.entries.len() {
            if matches!(self.entries[index].state, State::Running { due_ms, .. } if due_ms <= now_ms)
            {
                self.step(index, now_ms, &mut started);
            }
            index += 1;
        }
        started
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
    /// start; step k shows frame k - 1 (an animation) or node k - 1 (a
    /// move). The step after an animation's last frame, or the one that
    /// reaches a move's last node, finishes the entry and examines again.
    fn step(&mut self, index: usize, now_ms: u64, started: &mut Vec<Started>) {
        self.examine(now_ms, started);
        let entry = &mut self.entries[index];
        let State::Running { steps, due_ms } = &mut entry.state else {
            return;
        };
        *steps += 1;
        let step = *steps;
        let display = self.displays.get_mut(&entry.owner);
        let finished = match &entry.action {
            Action::Animate { art, frames, hide } => {
                *due_ms += frames.interval_ms.max(1);
                if let Some(shift) = frames.shifts.get(step - 1) {
                    if let Some(display) = display {
                        display.art = *art;
                        display.frame = step - 1;
                        display.hidden = false;
                        display.displacement[0] += shift[0];
                        display.displacement[1] += shift[1];
                    }
                    false
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
                    true
                }
            }
            Action::Move { art, frames, nodes } => {
                *due_ms += frames.interval_ms.max(1);
                if let (Some(display), Some((hex, offset))) = (display, nodes.get(step - 1)) {
                    display.art = *art;
                    display.frame = (step - 1) % frames.shifts.len().max(1);
                    display.hex = Some(*hex);
                    display.displacement = *offset;
                }
                step >= nodes.len()
            }
            _ => true,
        };
        if finished {
            entry.state = State::Finished;
            self.examine(now_ms, started);
        }
    }
    /// Run every remaining step at once (headless playback, and the final
    /// state an attack leaves). Returns what started, in order.
    pub fn run_to_end(&mut self, mut now_ms: u64) -> Vec<Started> {
        let mut started = Vec::new();
        if !self.registered {
            started.extend(self.register(now_ms));
        }
        for _ in 0..100_000 {
            let Some(due) = self.due_ms() else {
                break;
            };
            now_ms = now_ms.max(due);
            started.extend(self.pass(now_ms));
        }
        started
    }
}

/// An instant action's effect on its owner's display.
fn apply(display: &mut Display, action: &Action) {
    match action {
        Action::Turn { orientation, art } => {
            display.orientation = *orientation;
            display.art = *art;
            display.frame = 0;
            display.displacement = [0; 2];
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
