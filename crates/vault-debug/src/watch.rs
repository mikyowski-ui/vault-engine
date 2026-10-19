//! Script variable watches (GVAR, MVAR, LVAR) with per-tick change tracking.
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};
use vault_core::WorldState;

/// The three script variable pools of `WorldState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VarScope {
    /// `WorldState::gvars`, the campaign globals (absent before the
    /// campaign initializer loads).
    Global,
    /// `WorldState::mvars`, the current map's variables.
    Map,
    /// `WorldState::lvars`, the shared pool of script locals.
    Local,
}
impl VarScope {
    pub fn name(self) -> &'static str {
        match self {
            Self::Global => "GVAR",
            Self::Map => "MVAR",
            Self::Local => "LVAR",
        }
    }
    fn pool(self, world: &WorldState) -> &[i32] {
        match self {
            Self::Global => world.gvars.as_deref().unwrap_or_default(),
            Self::Map => &world.mvars,
            Self::Local => &world.lvars,
        }
    }
}
impl fmt::Display for VarScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One watchable variable slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VarAddress {
    Global(usize),
    Map(usize),
    /// A flat index into the LVAR pool.
    Local(usize),
    /// Local `index` of script `script`: its slice of the LVAR pool starts at
    /// `ScriptInstance::local_base` and holds `local_count` slots. Absent
    /// while the script is missing or has no allocated locals.
    ScriptLocal {
        script: u32,
        index: usize,
    },
}
impl VarAddress {
    pub fn scope(self) -> VarScope {
        match self {
            Self::Global(_) => VarScope::Global,
            Self::Map(_) => VarScope::Map,
            Self::Local(_) | Self::ScriptLocal { .. } => VarScope::Local,
        }
    }
    /// The LVAR pool position a script-local address refers to now.
    pub fn pool_index(self, world: &WorldState) -> Option<usize> {
        match self {
            Self::Global(index) | Self::Map(index) | Self::Local(index) => Some(index),
            Self::ScriptLocal { script, index } => {
                let instance = world.scripts.get(&script)?;
                let base = usize::try_from(instance.local_base).ok()?;
                (index < instance.local_count).then(|| base + index)
            }
        }
    }
    /// The current value, or `None` when the slot does not exist.
    pub fn read(self, world: &WorldState) -> Option<i32> {
        let index = self.pool_index(world)?;
        self.scope().pool(world).get(index).copied()
    }
}
impl fmt::Display for VarAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global(index) | Self::Map(index) | Self::Local(index) => {
                write!(f, "{}[{index}]", self.scope())
            }
            Self::ScriptLocal { script, index } => write!(f, "LVAR[{script:#x}:{index}]"),
        }
    }
}

/// Which variables to watch: explicit slots (optionally labelled) and whole
/// scopes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchList {
    slots: BTreeMap<VarAddress, Option<String>>,
    scopes: BTreeSet<VarScope>,
}
impl WatchList {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn watch(&mut self, address: VarAddress) -> &mut Self {
        self.slots.entry(address).or_insert(None);
        self
    }
    pub fn watch_labeled(&mut self, address: VarAddress, label: impl Into<String>) -> &mut Self {
        self.slots.insert(address, Some(label.into()));
        self
    }
    /// Watch every slot of a pool, however long it grows.
    pub fn watch_all(&mut self, scope: VarScope) -> &mut Self {
        self.scopes.insert(scope);
        self
    }
    pub fn unwatch(&mut self, address: VarAddress) -> bool {
        self.slots.remove(&address).is_some()
    }
    pub fn unwatch_all(&mut self, scope: VarScope) -> bool {
        self.scopes.remove(&scope)
    }
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty() && self.scopes.is_empty()
    }
    pub fn label(&self, address: VarAddress) -> Option<&str> {
        self.slots.get(&address)?.as_deref()
    }
    /// The watched slots that exist now, with their values. Whole-scope
    /// watches contribute flat addresses (`Global`, `Map`, `Local`).
    pub fn snapshot(&self, world: &WorldState) -> VarSnapshot {
        let mut values = BTreeMap::new();
        for scope in &self.scopes {
            for (index, value) in scope.pool(world).iter().enumerate() {
                let address = match scope {
                    VarScope::Global => VarAddress::Global(index),
                    VarScope::Map => VarAddress::Map(index),
                    VarScope::Local => VarAddress::Local(index),
                };
                values.insert(address, *value);
            }
        }
        for address in self.slots.keys() {
            if let Some(value) = address.read(world) {
                values.insert(*address, value);
            }
        }
        VarSnapshot { values }
    }
}

/// Watched values at one tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VarSnapshot {
    pub values: BTreeMap<VarAddress, i32>,
}
impl VarSnapshot {
    pub fn get(&self, address: VarAddress) -> Option<i32> {
        self.values.get(&address).copied()
    }
    /// Changes from `self` (older) to `newer`, in address order.
    pub fn diff(&self, newer: &Self, list: &WatchList) -> Vec<VarChange> {
        let addresses: BTreeSet<_> = self.values.keys().chain(newer.values.keys()).collect();
        addresses
            .into_iter()
            .filter_map(|address| {
                let (old, new) = (self.get(*address), newer.get(*address));
                (old != new).then(|| VarChange {
                    address: *address,
                    label: list.label(*address).map(str::to_string),
                    old,
                    new,
                })
            })
            .collect()
    }
}

/// How a watched slot changed between two ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Changed,
    /// The slot came into existence (pool growth, a script's locals being
    /// allocated, or a watch added since the previous tick).
    Appeared,
    Disappeared,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarChange {
    pub address: VarAddress,
    pub label: Option<String>,
    pub old: Option<i32>,
    pub new: Option<i32>,
}
impl VarChange {
    pub fn kind(&self) -> ChangeKind {
        match (self.old, self.new) {
            (None, _) => ChangeKind::Appeared,
            (_, None) => ChangeKind::Disappeared,
            _ => ChangeKind::Changed,
        }
    }
}
impl fmt::Display for VarChange {
    /// `GVAR[12] (label): 0 -> 1`, with `-` for an absent side.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let side = |value: Option<i32>| value.map_or_else(|| "-".to_string(), |v| v.to_string());
        write!(f, "{}", self.address)?;
        if let Some(label) = &self.label {
            write!(f, " ({label})")?;
        }
        write!(f, ": {} -> {}", side(self.old), side(self.new))
    }
}

/// Tracks a watch list across ticks.
#[derive(Debug, Clone, Default)]
pub struct Watcher {
    pub list: WatchList,
    previous: Option<VarSnapshot>,
}
impl Watcher {
    pub fn new(list: WatchList) -> Self {
        Self {
            list,
            previous: None,
        }
    }
    /// Snapshot the watched variables and return what changed since the
    /// previous tick. The first tick (and the first after `reset`) only
    /// records a baseline and reports nothing.
    pub fn tick(&mut self, world: &WorldState) -> Vec<VarChange> {
        let current = self.list.snapshot(world);
        let changes = self
            .previous
            .as_ref()
            .map(|previous| previous.diff(&current, &self.list))
            .unwrap_or_default();
        self.previous = Some(current);
        changes
    }
    /// The snapshot taken by the latest tick.
    pub fn current(&self) -> Option<&VarSnapshot> {
        self.previous.as_ref()
    }
    /// Forget the baseline; the next tick reports nothing.
    pub fn reset(&mut self) {
        self.previous = None;
    }
}

/// One `ADDRESS = VALUE (label)` line per watched slot.
pub fn render_vars(snapshot: &VarSnapshot, list: &WatchList) -> String {
    let mut text = String::new();
    for (address, value) in &snapshot.values {
        text.push_str(&format!("{address} = {value}"));
        if let Some(label) = list.label(*address) {
            text.push_str(&format!(" ({label})"));
        }
        text.push('\n');
    }
    text
}
