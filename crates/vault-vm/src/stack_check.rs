//! SPEC-017 §8: the script stack model. A decoded program's units (the
//! initial stream, and each distinct procedure body) are run through an
//! abstract model of the value-stack depth (§8.2), with the host words
//! popping and pushing by a profile's table: Fallout 1's (SPEC-005), or
//! Fallout 2's (SPEC-005 as amended by SPEC-017), plus the versioned
//! enhanced range when the program is parsed enhanced. Revision 2: host
//! results that statement calls leave on the stack are absorbed where paths
//! rejoin. Signature inference (§8.3) tries other signatures for one word.
//!
//! `asset-audit --report-script-stack` runs it over whole corpora; the
//! engine runs [`check`] on each `.INT` a mod layer supplies before it
//! enters the VM. Neither prints script bytes, only counts and words.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{IntProgram, WordSet, extensions};

/// §8.2 the deepest model stack: 341 complete values (SPEC-004 §3.1).
const MAX_DEPTH: usize = 341;
/// §8.3 the candidate signatures: pops 0-8, push 0 or 1.
const MAX_POPS: u8 = 8;
/// The host words inference covers (§8.3).
pub const GAME_WORDS: std::ops::RangeInclusive<u16> = 0x80a1..=0x8183;

/// A unit (§8.2): its start address, entry depth and declared argument
/// count.
pub type Unit = (u32, usize, usize);

/// A §8.2 finding kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Finding {
    Underflow,
    Overflow,
    MergeConflict,
    Unresolved,
    RefusedWord,
    ReservedWord,
    UnexpectedFrame,
    DataDependent,
}
impl Finding {
    pub const ALL: [Self; 8] = [
        Self::Underflow,
        Self::Overflow,
        Self::MergeConflict,
        Self::Unresolved,
        Self::RefusedWord,
        Self::ReservedWord,
        Self::UnexpectedFrame,
        Self::DataDependent,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Underflow => "underflow",
            Self::Overflow => "overflow",
            Self::MergeConflict => "merge conflict",
            Self::Unresolved => "unresolved",
            Self::RefusedWord => "refused word",
            Self::ReservedWord => "reserved word",
            Self::UnexpectedFrame => "unexpected frame",
            Self::DataDependent => "data-dependent",
        }
    }
}

/// A host word's effect on the model: pops and push, or `addregion`'s
/// variable count; `None` for a reserved or unregistered word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Effect {
    Fixed { pops: u8, push: u8 },
    Variable,
}

/// The profile table the model reads host words from, with the signature
/// overrides inference tries.
#[derive(Debug, Clone)]
pub struct Table {
    pub words: WordSet,
    /// The versioned enhanced range ([`extensions::ENHANCED_RANGE`]) is
    /// accepted, its words taking their [`extensions::ENHANCED`] rows; an
    /// unassigned word of the range is a reserved word.
    pub enhanced: bool,
    overrides: BTreeMap<u16, (u8, u8)>,
}
impl Table {
    pub fn new(words: WordSet) -> Self {
        Self {
            words,
            enhanced: false,
            overrides: BTreeMap::new(),
        }
    }
    /// The table a program parsed under `words` uses, with the enhanced
    /// range when it was parsed `enhanced`.
    pub fn with_enhanced(words: WordSet, enhanced: bool) -> Self {
        Self {
            enhanced,
            ..Self::new(words)
        }
    }
    /// The table with `word`'s signature replaced by `signature` (§8.3).
    pub fn with(&self, word: u16, signature: (u8, u8)) -> Self {
        let mut table = self.clone();
        table.overrides.insert(word, signature);
        table
    }
    /// Whether the word set, or the enhanced range when it is on, holds
    /// `word`.
    pub fn accepts(&self, word: u16) -> bool {
        self.words.accepts_in(word, self.enhanced)
    }
    /// The row of a host word: the profile's, or under enhanced the
    /// enhanced table's for a word of that range.
    pub fn row(&self, word: u16) -> Option<&'static extensions::OpcodeSpec> {
        if self.enhanced && extensions::ENHANCED_RANGE.contains(&word) {
            return extensions::enhanced_opcode(word);
        }
        extensions::opcode_in(&self.words, word)
    }
    fn effect(&self, word: u16) -> Option<Effect> {
        if let Some((pops, push)) = self.overrides.get(&word) {
            return Some(Effect::Fixed {
                pops: *pops,
                push: *push,
            });
        }
        let spec = self.row(word).filter(|spec| spec.registered)?;
        Some(if spec.variable {
            Effect::Variable
        } else {
            Effect::Fixed {
                pops: spec.operands.len() as u8,
                push: u8::from(spec.push != extensions::PushKind::None),
            }
        })
    }
    /// The table signature of a host word, as `(pops, push)`.
    pub fn signature(&self, word: u16) -> Option<(u8, u8)> {
        match self.effect(word)? {
            Effect::Fixed { pops, push } => Some((pops, push)),
            Effect::Variable => None,
        }
    }
}

/// The code the model walks: an image, its instruction boundaries of the
/// linear decode, and the procedure body addresses.
#[derive(Debug, Clone)]
pub struct Image {
    /// The script's name, lowercase, as `scripts.lst` lists it (§8.4).
    pub name: String,
    pub bytes: Vec<u8>,
    boundaries: BTreeSet<u32>,
    bodies: BTreeSet<u32>,
    /// The units: (start address, entry depth, declared argument count).
    pub units: Vec<Unit>,
    /// Procedure bodies with no instruction to run (imported records, or
    /// a body at the image end).
    pub empty_bodies: usize,
}
fn instruction_length(word: u16) -> u32 {
    if word & 0x3ff == 1 { 6 } else { 2 }
}
fn linear(bytes: &[u8], ranges: &[(usize, usize)]) -> BTreeSet<u32> {
    let mut boundaries = BTreeSet::new();
    for (start, end) in ranges {
        let mut at = *start;
        while at + 2 <= *end {
            boundaries.insert(at as u32);
            at += instruction_length(u16::from_be_bytes([bytes[at], bytes[at + 1]])) as usize;
        }
    }
    boundaries
}
impl Image {
    /// A program's image: the initial stream unit at offset 0, depth 0,
    /// and one unit per distinct procedure body, at its declared argument
    /// count plus 1.
    pub fn of(name: String, program: &IntProgram) -> Self {
        let bytes = program.image().to_vec();
        let boundaries = linear(&bytes, &[(0, 42), (program.code_start, bytes.len())]);
        let mut units = vec![(0, 0, 0)];
        let mut bodies = BTreeSet::new();
        let mut empty_bodies = 0;
        for procedure in program.procedures() {
            if procedure.flags & crate::procedure_flags::IMPORT != 0
                || !program.boundary(procedure.body)
            {
                empty_bodies += 1;
                continue;
            }
            if bodies.insert(procedure.body) {
                let arguments = procedure.arguments as usize;
                units.push((procedure.body, arguments + 1, arguments));
            }
        }
        Self {
            name,
            bytes,
            boundaries,
            bodies,
            units,
            empty_bodies,
        }
    }
    /// A bare procedure body at offset 0 with `arguments` declared (§8.6).
    pub fn body(bytes: &[u8], arguments: usize) -> Self {
        Self {
            name: String::new(),
            boundaries: linear(bytes, &[(0, bytes.len())]),
            bytes: bytes.to_vec(),
            bodies: BTreeSet::from([0]),
            units: vec![(0, arguments + 1, arguments)],
            empty_bodies: 0,
        }
    }
    /// The instruction word at each boundary of the linear decode, in
    /// address order.
    pub fn linear_words(&self) -> impl Iterator<Item = u16> + '_ {
        self.boundaries
            .iter()
            .filter_map(|at| self.word(*at).map(|(word, _)| word))
    }
    fn word(&self, at: u32) -> Option<(u16, Option<u32>)> {
        let at = at as usize;
        let word = u16::from_be_bytes(self.bytes.get(at..at + 2)?.try_into().ok()?);
        let payload = if instruction_length(word) == 6 {
            Some(u32::from_be_bytes(
                self.bytes.get(at + 2..at + 6)?.try_into().ok()?,
            ))
        } else {
            None
        };
        Some((word, payload))
    }
}

/// A model slot (§8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// The tag and payload of the slot-1 push that produced it directly.
    Pushed(u16, u32),
    /// A value a host word's row pushes (Revision 2).
    Host,
    /// Every other value, including a procedure call's and a core word's
    /// result.
    Unknown,
}
fn known(slot: Slot) -> Option<i32> {
    match slot {
        Slot::Pushed(tag, payload) if tag & 0xf7ff == 0xc001 => Some(payload as i32),
        _ => None,
    }
}

/// §8.2 "Leaked results" steps 1-4: an arrival's stack met with the
/// recorded one, and whether step 3 removed a host result; `None` for a
/// merge conflict (step 2).
fn meet(recorded: &[Slot], arrival: &[Slot]) -> Option<(Vec<Slot>, bool)> {
    let (deeper, shallower) = if arrival.len() > recorded.len() {
        (arrival, recorded)
    } else {
        (recorded, arrival)
    };
    let mut k = deeper.len() - shallower.len();
    if deeper.iter().filter(|slot| **slot == Slot::Host).count() < k {
        return None;
    }
    let leaked = k > 0;
    // The k lowest host results, nearest the bottom, are the leaked ones.
    let remainder = deeper.iter().filter(|slot| {
        let leak = k > 0 && **slot == Slot::Host;
        k -= usize::from(leak);
        !leak
    });
    let met = remainder
        .zip(shallower)
        .map(|(a, b)| if a == b { *a } else { Slot::Unknown })
        .collect();
    Some((met, leaked))
}

/// What one unit's analysis found.
#[derive(Debug, Clone, Default)]
pub struct UnitReport {
    /// Each finding with the word involved (0 for none): at most one of a
    /// kind per instruction address, in address order.
    pub findings: Vec<(Finding, u16)>,
    /// The host words reached, by occurrence.
    pub reached: BTreeMap<u16, usize>,
    /// Per host word and operand position (first popped first): direct
    /// constant pushes, and how many of them were static strings.
    pub operands: BTreeMap<(u16, usize), [u64; 2]>,
    /// Whether a merge removed a leaked host result (a unit with leaked
    /// results).
    pub leaked: bool,
}

/// The state recorded at an address: the model stack, and whether the
/// path has run its frame word (`0x802B`).
#[derive(Debug, Clone)]
struct State {
    stack: Vec<Slot>,
    framed: bool,
}

/// One unit's exploration: the state recorded per address, and the
/// worklist of addresses to explore from their recorded state.
struct Walk {
    states: BTreeMap<u32, State>,
    work: VecDeque<u32>,
    queued: BTreeSet<u32>,
    breadth_first: bool,
    found: BTreeMap<(u32, Finding), u16>,
    leaked: bool,
}
impl Walk {
    fn find(&mut self, at: u32, finding: Finding, word: u16) {
        self.found.entry((at, finding)).or_insert(word);
    }
    fn queue(&mut self, at: u32) {
        if self.queued.insert(at) {
            self.work.push_back(at);
        }
    }
    fn next(&mut self) -> Option<u32> {
        let at = if self.breadth_first {
            self.work.pop_front()
        } else {
            self.work.pop_back()
        }?;
        self.queued.remove(&at);
        Some(at)
    }
    /// §8.2 a path arriving at `at`: recorded on the first arrival, else
    /// met with the recorded state, which a changed state replaces and
    /// explores again (step 5). The frame flag stays the first arrival's.
    fn arrive(&mut self, at: u32, arrival: State) {
        let Some(recorded) = self.states.get_mut(&at) else {
            self.states.insert(at, arrival);
            self.queue(at);
            return;
        };
        let Some((stack, leaked)) = meet(&recorded.stack, &arrival.stack) else {
            self.find(at, Finding::MergeConflict, 0);
            return;
        };
        self.leaked |= leaked;
        if stack != recorded.stack {
            recorded.stack = stack;
            self.queue(at);
        }
    }
}

/// §8.2 one unit from `start` at depth `depth`.
pub fn analyse_unit(image: &Image, table: &Table, unit: Unit) -> UnitReport {
    explore(image, table, unit, false)
}

/// §8.2 one unit, exploring the worklist's newest address first, or with
/// `breadth_first` its oldest.
fn explore(
    image: &Image,
    table: &Table,
    (start, depth, arguments): Unit,
    breadth_first: bool,
) -> UnitReport {
    let mut walk = Walk {
        states: BTreeMap::new(),
        work: VecDeque::new(),
        queued: BTreeSet::new(),
        breadth_first,
        found: BTreeMap::new(),
        leaked: false,
    };
    walk.arrive(
        start,
        State {
            stack: vec![Slot::Unknown; depth],
            framed: false,
        },
    );
    // Per host word address, from its latest exploration: the word, and
    // per operand position whether a direct push supplied it and whether
    // that was a static string.
    let mut evidence: BTreeMap<u32, (u16, Vec<Option<bool>>)> = BTreeMap::new();
    let mut reached: BTreeMap<u32, u16> = BTreeMap::new();
    while let Some(at) = walk.next() {
        let State {
            mut stack,
            mut framed,
        } = walk.states[&at].clone();
        let Some((word, payload)) = image.word(at) else {
            continue;
        };
        let next = at + instruction_length(word);
        macro_rules! pop {
            ($count:expr) => {{
                let count: usize = $count;
                if stack.len() < count {
                    walk.find(at, Finding::Underflow, word);
                    continue;
                }
                let popped = stack.split_off(stack.len() - count);
                popped
            }};
        }
        // A jump target: a known integer at an instruction boundary of the
        // linear decode.
        let target = |slot: Slot| {
            known(slot)
                .and_then(|address| u32::try_from(address).ok())
                .filter(|address| image.boundaries.contains(address))
        };
        if word & 0x3ff == 1 {
            if !table.accepts(word) {
                walk.find(at, Finding::RefusedWord, word);
                continue;
            }
            stack.push(payload.map_or(Slot::Unknown, |payload| Slot::Pushed(word, payload)));
        } else if !table.accepts(word) {
            walk.find(at, Finding::RefusedWord, word);
            continue;
        } else {
            match word {
                0x8000 | 0x8002 | 0x8003 | 0x8019 | 0x801e | 0x8029 | 0x802c | 0x8049 | 0x804a
                | 0x804b | 0x800f => {}
                0x800c => stack.push(Slot::Unknown),
                0x8012 | 0x8014 | 0x8028 | 0x802d | 0x8032 | 0x8043..=0x8046 => {
                    pop!(1);
                    stack.push(Slot::Unknown);
                }
                0x800d | 0x801a | 0x8016 | 0x8047 | 0x8048 | 0x8008 | 0x800a | 0x800b => {
                    pop!(1);
                }
                0x8013 | 0x8015 | 0x8017 | 0x8027 | 0x8031 | 0x8006 | 0x8007 => {
                    pop!(2);
                }
                0x8018 => {
                    let mut pair = pop!(2);
                    pair.reverse();
                    stack.extend(pair);
                }
                0x801b => {
                    let top = pop!(1)[0];
                    stack.extend([top, top]);
                }
                0x8033..=0x8042 => {
                    pop!(2);
                    stack.push(Slot::Unknown);
                }
                0x801f => {
                    pop!(3);
                }
                0x802b => {
                    pop!(1);
                    if framed {
                        walk.find(at, Finding::UnexpectedFrame, word);
                        continue;
                    }
                    framed = true;
                    stack.resize(arguments, Slot::Unknown);
                }
                0x802a => stack.clear(),
                0x8004 => {
                    let slot = pop!(1)[0];
                    let Some(target) = target(slot) else {
                        walk.find(at, Finding::Unresolved, word);
                        continue;
                    };
                    if !image.bodies.contains(&target) {
                        walk.arrive(target, State { stack, framed });
                    }
                    continue;
                }
                0x802f => {
                    let popped = pop!(2);
                    let Some(target) = target(popped[0]) else {
                        walk.find(at, Finding::Unresolved, word);
                        continue;
                    };
                    let branch = State {
                        stack: stack.clone(),
                        framed,
                    };
                    walk.arrive(target, branch);
                }
                0x8030 => {
                    if stack.len() < 2 {
                        walk.find(at, Finding::Underflow, word);
                        continue;
                    }
                    let Some(target) = target(stack[stack.len() - 2]) else {
                        walk.find(at, Finding::Unresolved, word);
                        continue;
                    };
                    let branch = State {
                        stack: stack[..stack.len() - 2].to_vec(),
                        framed,
                    };
                    walk.arrive(target, branch);
                    stack.pop();
                }
                0x8005 => {
                    // Only the argument count, below the index, must be
                    // known (Revision 2).
                    let popped = pop!(2);
                    let Some(count) =
                        known(popped[0]).and_then(|count| usize::try_from(count).ok())
                    else {
                        walk.find(at, Finding::Unresolved, word);
                        continue;
                    };
                    pop!(count);
                    stack.push(Slot::Unknown);
                }
                0x802e => {
                    walk.find(at, Finding::DataDependent, word);
                    continue;
                }
                0x8009 => {
                    pop!(1);
                    continue;
                }
                0x800e | 0x8010 | 0x8011 | 0x801c | 0x801d | 0x8020..=0x8026 => continue,
                0x804c..=0xffff => {
                    reached.insert(at, word);
                    match table.effect(word) {
                        None => {
                            walk.find(at, Finding::ReservedWord, word);
                            continue;
                        }
                        Some(Effect::Variable) => {
                            let Some(count) = stack
                                .last()
                                .copied()
                                .and_then(known)
                                .and_then(|count| usize::try_from(count).ok())
                            else {
                                walk.find(at, Finding::Unresolved, word);
                                continue;
                            };
                            pop!(1);
                            pop!(count);
                        }
                        Some(Effect::Fixed { pops, push }) => {
                            let popped = pop!(usize::from(pops));
                            let operands = popped
                                .iter()
                                .rev()
                                .map(|slot| match slot {
                                    Slot::Pushed(tag, _) => Some(*tag == 0x9001),
                                    _ => None,
                                })
                                .collect();
                            evidence.insert(at, (word, operands));
                            stack.extend(std::iter::repeat_n(Slot::Host, usize::from(push)));
                        }
                    }
                }
                _ => {
                    walk.find(at, Finding::ReservedWord, word);
                    continue;
                }
            }
        }
        if stack.len() > MAX_DEPTH {
            walk.find(at, Finding::Overflow, word);
            continue;
        }
        walk.arrive(next, State { stack, framed });
    }
    let mut report = UnitReport {
        findings: walk
            .found
            .into_iter()
            .map(|((_, finding), word)| (finding, word))
            .collect(),
        leaked: walk.leaked,
        ..UnitReport::default()
    };
    for word in reached.values() {
        *report.reached.entry(*word).or_default() += 1;
    }
    for (word, operands) in evidence.values() {
        for (position, operand) in operands.iter().enumerate() {
            if let Some(string) = operand {
                let counts = report.operands.entry((*word, position)).or_default();
                counts[0] += 1;
                counts[1] += u64::from(*string);
            }
        }
    }
    report
}

/// §8.4 the findings recorded exceptions excuse, by script name: kind and
/// word.
pub type Excused = BTreeMap<String, BTreeSet<(Finding, u16)>>;
/// Whether a unit's findings are all excused (§8.3: a recorded exception
/// does not make a unit count as one with a finding).
pub fn clean(image: &Image, report: &UnitReport, excused: &Excused) -> bool {
    let excused = excused.get(&image.name);
    report
        .findings
        .iter()
        .all(|key| excused.is_some_and(|keys| keys.contains(key)))
}

/// §8.3 the candidates for `word` under which none of `units` has a
/// finding, apart from `excused` ones.
pub fn consistent(
    units: &[(&Image, Unit)],
    table: &Table,
    word: u16,
    excused: &Excused,
) -> Vec<(u8, u8)> {
    let mut candidates = Vec::new();
    for pops in 0..=MAX_POPS {
        for push in 0..=1 {
            let table = table.with(word, (pops, push));
            if units
                .iter()
                .all(|(image, unit)| clean(image, &analyse_unit(image, &table, *unit), excused))
            {
                candidates.push((pops, push));
            }
        }
    }
    candidates
}

/// §8.4 a recorded exception: a finding in game data that the model
/// correctly reports, matched exactly by the profile table (the list it is
/// in), script, finding kind, word and count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exception {
    pub script: &'static str,
    pub finding: Finding,
    pub word: u16,
    pub count: u64,
}
/// §8.4 the `fallout1` table's recorded exceptions: `gatedemo.int`'s one
/// `start_gdialog` pushes four operands, not five. The `fallout2` table
/// has none.
pub const FALLOUT1_EXCEPTIONS: [Exception; 1] = [Exception {
    script: "gatedemo.int",
    finding: Finding::Underflow,
    word: 0x80de,
    count: 1,
}];
/// The recorded exceptions of `words`' table.
pub fn exceptions(words: &WordSet) -> &'static [Exception] {
    if words.fallout2() {
        &[]
    } else {
        &FALLOUT1_EXCEPTIONS
    }
}

/// The findings of each unit of `image` under `table`.
pub fn analyse(image: &Image, table: &Table) -> Vec<UnitReport> {
    image
        .units
        .iter()
        .map(|unit| analyse_unit(image, table, *unit))
        .collect()
}

/// §8.4 the findings of `exceptions` that `images` match exactly, with
/// each exception's observed count (`None` when its script is not among
/// them). `reports` are the images' unit reports, in order.
pub fn excuse(
    images: &[Image],
    reports: &[Vec<UnitReport>],
    exceptions: &[Exception],
) -> (Excused, Vec<(Exception, Option<u64>)>) {
    let mut excused = Excused::new();
    let mut observed_counts = Vec::new();
    for exception in exceptions {
        let observed = images
            .iter()
            .zip(reports)
            .find(|(image, _)| image.name == exception.script)
            .map(|(_, reports)| {
                reports
                    .iter()
                    .flat_map(|report| &report.findings)
                    .filter(|key| **key == (exception.finding, exception.word))
                    .count() as u64
            });
        if observed == Some(exception.count) {
            excused
                .entry(exception.script.to_string())
                .or_default()
                .insert((exception.finding, exception.word));
        }
        observed_counts.push((*exception, observed));
    }
    (excused, observed_counts)
}

/// One program's findings under `table` by kind and word (0 for none),
/// with counts, apart from those its matched recorded `exceptions` excuse
/// (by `name`, the script's lowercase file name). Empty for a clean
/// program.
pub fn check(
    name: &str,
    program: &IntProgram,
    table: &Table,
    exceptions: &[Exception],
) -> BTreeMap<(Finding, u16), u64> {
    let image = Image::of(name.to_ascii_lowercase(), program);
    let reports = analyse(&image, table);
    let (excused, _) = excuse(
        std::slice::from_ref(&image),
        std::slice::from_ref(&reports),
        exceptions,
    );
    let excused = excused.get(&image.name);
    let mut findings = BTreeMap::new();
    for key in reports.iter().flat_map(|report| &report.findings) {
        if !excused.is_some_and(|keys| keys.contains(key)) {
            *findings.entry(*key).or_default() += 1;
        }
    }
    findings
}

/// `findings` as text, by kind and word: `underflow at word 0x801a x1,
/// merge conflict x2`.
pub fn summary(findings: &BTreeMap<(Finding, u16), u64>) -> String {
    findings
        .iter()
        .map(|((kind, word), count)| {
            if *word == 0 {
                format!("{} x{count}", kind.name())
            } else {
                format!("{} at word {word:#06x} x{count}", kind.name())
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        text.split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect()
    }
    /// SPEC-017 §8.6 Vector N.
    fn vector_n() -> Image {
        Image::body(
            &hex("80 2B C0 01 00 00 00 30 C0 01 00 00 00 01 80 2F \
                 C0 01 00 00 00 00 C0 01 00 00 00 05 C0 01 00 00 \
                 00 01 C0 01 00 00 4E 84 C0 01 00 00 00 02 80 E6 \
                 80 2A 80 29 80 1C"),
            0,
        )
    }

    #[test]
    fn spec017_vector_n_the_fallout2_table_has_no_finding() {
        let image = vector_n();
        let table = Table::new(WordSet::FALLOUT2);
        let report = analyse_unit(&image, &table, image.units[0]);
        assert!(report.findings.is_empty(), "{:?}", report.findings);
        assert_eq!(report.reached.get(&0x80e6), Some(&1));
        // The five operands of set_exit_grids were direct integer pushes.
        for position in 0..5 {
            assert_eq!(report.operands[&(0x80e6, position)], [1, 0]);
        }
    }

    #[test]
    fn spec017_vector_n_the_fallout1_table_merges_at_0x30_and_infers_5_0_and_5_1() {
        let image = vector_n();
        let table = Table::new(WordSet::FALLOUT1);
        // barter_asking leaves two known constants and no host result:
        // a merge conflict, whichever successor is explored first.
        for breadth_first in [false, true] {
            let report = explore(&image, &table, image.units[0], breadth_first);
            assert_eq!(report.findings, [(Finding::MergeConflict, 0)]);
            assert!(!report.leaked);
        }
        // Inference: pops 5 with push 0, and with push 1 (the result is
        // never consumed, so it leaks); pops 6, push 1 underflows.
        let units = [(&image, image.units[0])];
        assert_eq!(
            consistent(&units, &table, 0x80e6, &Excused::new()),
            [(5, 0), (5, 1)]
        );
        let leak = analyse_unit(&image, &table.with(0x80e6, (5, 1)), image.units[0]);
        assert!(leak.findings.is_empty() && leak.leaked);
        let underflow = analyse_unit(&image, &table.with(0x80e6, (6, 1)), image.units[0]);
        assert_eq!(underflow.findings, [(Finding::Underflow, 0x80e6)]);
    }

    #[test]
    fn spec017_vector_o_absorbs_a_leaked_host_result_and_an_unknown_call_index() {
        let image = Image::body(
            &hex("80 2B C0 01 00 00 00 24 C0 01 00 00 00 01 80 2F \
                 C0 01 00 00 00 01 C0 01 00 00 00 02 C0 01 00 00 \
                 00 03 80 B6 C0 01 00 00 00 00 90 01 00 00 00 00 \
                 80 28 80 05 80 1A 80 2A 80 29 80 1C"),
            0,
        );
        let table = Table::new(WordSet::FALLOUT1);
        for breadth_first in [false, true] {
            let report = explore(&image, &table, image.units[0], breadth_first);
            assert!(report.findings.is_empty(), "{:?}", report.findings);
            assert!(report.leaked);
        }
    }

    #[test]
    fn a_merge_removes_the_lowest_host_results_and_meets_the_rest() {
        let constant = |value| Slot::Pushed(0xc001, value);
        // The lowest host result goes; the one above the constant stays.
        assert_eq!(
            meet(
                &[Slot::Host, constant(5), Slot::Host],
                &[constant(5), Slot::Host]
            ),
            Some((vec![constant(5), Slot::Host], true))
        );
        // The shallower state may be the recorded one.
        assert_eq!(
            meet(&[constant(5)], &[Slot::Host, constant(5)]),
            Some((vec![constant(5)], true))
        );
        // Equal depths: different values meet to unknown.
        assert_eq!(
            meet(&[constant(5), Slot::Host], &[constant(6), Slot::Host]),
            Some((vec![Slot::Unknown, Slot::Host], false))
        );
        // Too few host results: pushed operands and call results are not
        // absorbed.
        assert_eq!(meet(&[constant(5), Slot::Host], &[]), None);
        assert_eq!(meet(&[Slot::Unknown], &[]), None);
    }

    #[test]
    fn a_buried_leaked_result_keeps_the_known_jump_target_above_it() {
        // The then-arm leaves move_to's result below the end address and
        // jumps to 0x38; the else-arm pushes the end address alone and
        // falls through to it. 0x38 jumps to the address on top.
        let image = Image::body(
            &hex("80 2B C0 01 00 00 00 32 C0 01 00 00 00 01 80 2F \
                 C0 01 00 00 00 01 C0 01 00 00 00 02 C0 01 00 00 \
                 00 03 80 B6 C0 01 00 00 00 3A C0 01 00 00 00 38 \
                 80 04 C0 01 00 00 00 3A 80 04 80 2A 80 29 80 1C"),
            0,
        );
        let table = Table::new(WordSet::FALLOUT1);
        for breadth_first in [false, true] {
            let report = explore(&image, &table, image.units[0], breadth_first);
            assert!(report.findings.is_empty(), "{:?}", report.findings);
            assert!(report.leaked);
        }
    }

    #[test]
    fn a_call_needs_only_a_known_argument_count() {
        let table = Table::new(WordSet::FALLOUT1);
        let findings = |code: &str| {
            let image = Image::body(&hex(code), 0);
            analyse_unit(&image, &table, image.units[0]).findings
        };
        // The index comes from a name lookup: no finding.
        assert!(
            findings(
                "80 2B C0 01 00 00 00 00 90 01 00 00 00 00 80 28 80 05 80 1A \
                 80 2A 80 29 80 1C"
            )
            .is_empty()
        );
        // A computed count, or a host result as the count, is unresolved.
        assert_eq!(
            findings("80 2B C0 01 00 00 00 00 80 28 C0 01 00 00 00 07 80 05 80 1C"),
            [(Finding::Unresolved, 0x8005)]
        );
        assert_eq!(
            findings("80 2B 80 BC C0 01 00 00 00 07 80 05 80 1C"),
            [(Finding::Unresolved, 0x8005)]
        );
    }

    #[test]
    fn a_finding_is_reported_once_per_address_and_kind() {
        // The short arm reaches 0x26 first with a leaked self_obj result;
        // the long arm's arrival there without it re-explores 0x26, whose
        // data-dependent jump is reported once.
        let image = Image::body(
            &hex("80 2B C0 01 00 00 00 1A C0 01 00 00 00 01 80 2F \
                 80 BC C0 01 00 00 00 26 80 04 80 00 80 00 80 00 \
                 80 00 80 00 80 00 80 2E"),
            0,
        );
        let table = Table::new(WordSet::FALLOUT1);
        for breadth_first in [false, true] {
            let report = explore(&image, &table, image.units[0], breadth_first);
            assert_eq!(report.findings, [(Finding::DataDependent, 0x802e)]);
            assert!(report.leaked);
        }
    }

    #[test]
    fn the_model_reports_each_finding_kind() {
        let table = Table::new(WordSet::FALLOUT2);
        let findings = |code: &str, arguments: usize| {
            let image = Image::body(&hex(code), arguments);
            analyse_unit(&image, &table, image.units[0]).findings
        };
        // Underflow: a discard on the empty stack after the frame.
        assert_eq!(
            findings("80 2B 80 1A 80 1C", 0),
            [(Finding::Underflow, 0x801a)]
        );
        // A reserved Fallout 2 word ends the path.
        assert_eq!(
            findings("80 2B 81 5A 80 1C", 0),
            [(Finding::ReservedWord, 0x815a)]
        );
        // Outside the set: refused.
        assert_eq!(
            findings("80 2B 81 84 80 1C", 0),
            [(Finding::RefusedWord, 0x8184)]
        );
        // A second frame.
        assert_eq!(
            findings("80 2B C0 01 00 00 00 00 80 2B 80 1C", 0),
            [(Finding::UnexpectedFrame, 0x802b)]
        );
        // A computed jump target.
        assert_eq!(
            findings("80 2B C0 01 00 00 00 00 80 45 80 04", 0),
            [(Finding::Unresolved, 0x8004)]
        );
        // A target off the instruction boundaries.
        assert_eq!(
            findings("80 2B C0 01 00 00 00 03 80 04", 0),
            [(Finding::Unresolved, 0x8004)]
        );
        assert_eq!(
            findings("80 2B C0 01 00 00 00 00 80 2E", 0),
            [(Finding::DataDependent, 0x802e)]
        );
        // Overflow past 341 values.
        let pushes = "C0 01 00 00 00 00 ".repeat(342);
        assert_eq!(
            findings(&format!("80 2B {pushes}80 1C"), 0),
            [(Finding::Overflow, 0xc001)]
        );
        // A local call with known index and count: pops both, the
        // arguments, and pushes the result; the arguments of the frame
        // start below the count.
        assert!(
            findings(
                "80 2B C0 01 00 00 00 07 C0 01 00 00 00 01 C0 01 00 00 00 02 \
                 80 05 80 1A 80 2A 80 29 80 1C",
                0
            )
            .is_empty()
        );
        // A declared argument: entry depth 2, the frame leaves 1.
        assert!(findings("80 2B 80 1A 80 1C", 1).is_empty());
        // set_map_music's track operand is a string push (offset evidence).
        let image = Image::body(
            &hex("80 2B C0 01 00 00 00 03 90 01 00 00 00 06 80 E2 80 1C"),
            0,
        );
        let report = analyse_unit(&image, &table, image.units[0]);
        assert!(report.findings.is_empty());
        assert_eq!(report.operands[&(0x80e2, 0)], [1, 1]);
        assert_eq!(report.operands[&(0x80e2, 1)], [1, 0]);
    }

    #[test]
    fn the_enhanced_range_takes_its_rows_only_under_enhanced() {
        // engine_api_version pushes one value, discarded; op_cam_reset pops
        // one; 0x8201 is unassigned.
        let code = "80 2B 82 00 80 1A C0 01 00 00 00 05 82 13 80 1C";
        for words in [WordSet::FALLOUT1, WordSet::FALLOUT2] {
            let findings = |table: &Table, code: &str| {
                let image = Image::body(&hex(code), 0);
                analyse_unit(&image, table, image.units[0]).findings
            };
            let enhanced = Table::with_enhanced(words.clone(), true);
            assert!(findings(&enhanced, code).is_empty());
            assert_eq!(enhanced.signature(0x8200), Some((0, 1)));
            assert_eq!(enhanced.signature(0x8213), Some((1, 0)));
            assert_eq!(
                findings(&enhanced, "80 2B 82 01 80 1C"),
                [(Finding::ReservedWord, 0x8201)]
            );
            // Too few operands for op_cam_pan_to.
            assert_eq!(
                findings(&enhanced, "80 2B C0 01 00 00 00 05 82 10 80 1C"),
                [(Finding::Underflow, 0x8210)]
            );
            // Without enhanced the range is outside the set.
            let plain = Table::new(words);
            assert_eq!(findings(&plain, code), [(Finding::RefusedWord, 0x8200)]);
            assert_eq!(plain.signature(0x8200), None);
        }
    }

    #[test]
    fn check_counts_by_kind_and_word_and_applies_matched_exceptions() {
        // A `start` with a four-operand start_gdialog: one underflow,
        // gatedemo.int's recorded exception under the Fallout 1 table.
        let state = |code: Vec<u8>| {
            use vault_fixtures::state::{program, returning};
            let image = program(&[], &[(b"start".as_slice(), returning(code))]);
            IntProgram::parse_in(&image, &WordSet::FALLOUT2, false).unwrap()
        };
        let gdialog = state(hex(
            "C0 01 00 00 00 00 C0 01 00 00 00 00 C0 01 00 00 00 00 \
             C0 01 00 00 00 00 80 DE",
        ));
        let fallout1 = Table::new(WordSet::FALLOUT1);
        let findings = check("other.int", &gdialog, &fallout1, &FALLOUT1_EXCEPTIONS);
        assert_eq!(
            findings,
            BTreeMap::from([((Finding::Underflow, 0x80de), 1)])
        );
        assert_eq!(summary(&findings), "underflow at word 0x80de x1");
        assert!(
            check(
                "GATEDEMO.INT",
                &gdialog,
                &fallout1,
                exceptions(&WordSet::FALLOUT1)
            )
            .is_empty()
        );
        // The Fallout 2 table has no recorded exception.
        let fallout2 = Table::new(WordSet::FALLOUT2);
        assert_eq!(
            check(
                "gatedemo.int",
                &gdialog,
                &fallout2,
                exceptions(&WordSet::FALLOUT2)
            )
            .len(),
            1
        );
        let discard = state(hex("80 1A"));
        assert_eq!(
            summary(&check("a.int", &discard, &fallout1, &[])),
            "underflow at word 0x801a x1"
        );
    }
}
