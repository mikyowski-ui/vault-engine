//! Live map object and critter inspection with per-tick diffs.
use std::{collections::BTreeMap, fmt};
use vault_core::{ObjectRef, WorldState, world::WorldObject};
use vault_formats::{map::ObjectState, pro::Category};

/// Critter state, merged from the character sheet (when present) and the
/// map record's critter block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CritterSnapshot {
    /// The character sheet's current hit points, else the map record's.
    pub hp: i32,
    pub ap: i32,
    pub team: i32,
    /// AI packet number.
    pub ai: i32,
    pub combat_id: i32,
    /// The map record's damage-flag word.
    pub damage: i32,
    /// Object ID of the last attacker (-1 none).
    pub attacker: i32,
    pub poison: i32,
    pub radiation: i32,
    pub level: Option<i32>,
    pub alive: bool,
    pub player: bool,
    pub party: bool,
}

/// One world object at one tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectSnapshot {
    pub reference: ObjectRef,
    pub id: u32,
    pub pid: u32,
    pub category: Category,
    pub hex: Option<i32>,
    pub elevation: u8,
    pub orientation: u8,
    pub frame: usize,
    pub art_id: u32,
    pub flags: u32,
    pub script_id: i32,
    pub owner: Option<ObjectRef>,
    pub inventory: usize,
    pub name: String,
    pub active: bool,
    /// Present for critters: objects with a critter map block or a
    /// character sheet.
    pub critter: Option<CritterSnapshot>,
}
impl ObjectSnapshot {
    pub fn of(world: &WorldState, reference: ObjectRef, object: &WorldObject) -> Self {
        let raw = &object.raw;
        let state = match &raw.state {
            ObjectState::Critter(state) => Some(state),
            ObjectState::Other { .. } => None,
        };
        let sheet = object.character.as_ref();
        let critter = (state.is_some() || sheet.is_some()).then(|| CritterSnapshot {
            hp: sheet.map_or_else(|| state.map_or(0, |state| state.hp), |s| s.current_hp),
            ap: state.map_or(0, |state| state.ap),
            team: state.map_or(0, |state| state.team),
            ai: state.map_or(0, |state| state.ai),
            combat_id: raw.combat_id,
            damage: state.map_or(0, |state| state.damage),
            attacker: state.map_or(-1, |state| state.attacker),
            poison: sheet.map_or_else(|| state.map_or(0, |state| state.poison), |s| s.poison),
            radiation: sheet
                .map_or_else(|| state.map_or(0, |state| state.radiation), |s| s.radiation),
            level: sheet.map(|sheet| sheet.level),
            alive: object.alive(),
            player: reference == world.player,
            party: world.party.contains(&reference),
        });
        Self {
            reference,
            id: raw.id,
            pid: raw.pid,
            category: object.category(),
            hex: raw.hex,
            elevation: raw.elevation,
            orientation: raw.orientation,
            frame: raw.frame,
            art_id: raw.art_id,
            flags: raw.flags.0,
            script_id: raw.script_id,
            owner: object.owner,
            inventory: object.inventory.len(),
            name: String::from_utf8_lossy(&object.name).into_owned(),
            active: object.active,
            critter,
        }
    }
    /// Every inspectable field as `(name, text)`, in a stable order; diffs
    /// and detail views are built from this list.
    pub fn fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("id", self.id.to_string()),
            ("pid", format!("{:#010x}", self.pid)),
            ("category", category_name(self.category).to_string()),
            ("hex", optional(self.hex)),
            ("elevation", self.elevation.to_string()),
            ("orientation", self.orientation.to_string()),
            ("frame", self.frame.to_string()),
            ("art", format!("{:#010x}", self.art_id)),
            ("flags", format!("{:#010x}", self.flags)),
            ("script", self.script_id.to_string()),
            ("owner", optional(self.owner)),
            ("inventory", self.inventory.to_string()),
            ("name", self.name.clone()),
            ("active", self.active.to_string()),
        ];
        if let Some(critter) = &self.critter {
            fields.extend([
                ("hp", critter.hp.to_string()),
                ("ap", critter.ap.to_string()),
                ("team", critter.team.to_string()),
                ("ai", critter.ai.to_string()),
                ("combat_id", critter.combat_id.to_string()),
                ("damage", format!("{:#x}", critter.damage)),
                ("attacker", critter.attacker.to_string()),
                ("poison", critter.poison.to_string()),
                ("radiation", critter.radiation.to_string()),
                ("level", optional(critter.level)),
                ("alive", critter.alive.to_string()),
                ("player", critter.player.to_string()),
                ("party", critter.party.to_string()),
            ]);
        }
        fields
    }
}

fn optional(value: Option<impl fmt::Display>) -> String {
    value.map_or_else(|| "-".to_string(), |value| value.to_string())
}

pub fn category_name(category: Category) -> &'static str {
    match category {
        Category::Item => "item",
        Category::Critter => "critter",
        Category::Scenery => "scenery",
        Category::Wall => "wall",
        Category::Tile => "tile",
        Category::Misc => "misc",
    }
}

/// Which objects to inspect. The default selects every object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectFilter {
    pub elevation: Option<u8>,
    pub category: Option<Category>,
    pub critters_only: bool,
    pub reference: Option<ObjectRef>,
    /// Include inactive (removed-from-map) objects.
    pub include_inactive: bool,
}
impl ObjectFilter {
    pub fn all() -> Self {
        Self {
            include_inactive: true,
            ..Self::default()
        }
    }
    pub fn elevation(mut self, elevation: u8) -> Self {
        self.elevation = Some(elevation);
        self
    }
    pub fn category(mut self, category: Category) -> Self {
        self.category = Some(category);
        self
    }
    pub fn critters(mut self) -> Self {
        self.critters_only = true;
        self
    }
    pub fn reference(mut self, reference: ObjectRef) -> Self {
        self.reference = Some(reference);
        self
    }
    pub fn matches(&self, snapshot: &ObjectSnapshot) -> bool {
        self.elevation.is_none_or(|e| snapshot.elevation == e)
            && self.category.is_none_or(|c| snapshot.category == c)
            && (!self.critters_only || snapshot.critter.is_some())
            && self.reference.is_none_or(|r| snapshot.reference == r)
            && (self.include_inactive || snapshot.active)
    }
}

/// The matching objects, in reference order.
pub fn snapshot_objects(world: &WorldState, filter: &ObjectFilter) -> Vec<ObjectSnapshot> {
    world
        .iter()
        .filter(|(reference, _)| filter.reference.is_none_or(|r| r == *reference))
        .map(|(reference, object)| ObjectSnapshot::of(world, reference, object))
        .filter(|snapshot| filter.matches(snapshot))
        .collect()
}

/// One field that differs between ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    pub field: &'static str,
    pub old: String,
    pub new: String,
}

/// How an inspected object changed between ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectChange {
    /// Newly matching: created, or now passing the filter.
    Added(Box<ObjectSnapshot>),
    /// No longer matching: destroyed, or now outside the filter.
    Removed(Box<ObjectSnapshot>),
    Changed {
        reference: ObjectRef,
        name: String,
        fields: Vec<FieldChange>,
    },
}
impl ObjectChange {
    pub fn reference(&self) -> ObjectRef {
        match self {
            Self::Added(snapshot) | Self::Removed(snapshot) => snapshot.reference,
            Self::Changed { reference, .. } => *reference,
        }
    }
}
impl fmt::Display for ObjectChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added(snapshot) => write!(
                f,
                "+ {:#x} {} pid={:#010x} hex={}",
                snapshot.reference,
                display_name(&snapshot.name),
                snapshot.pid,
                optional(snapshot.hex)
            ),
            Self::Removed(snapshot) => write!(
                f,
                "- {:#x} {}",
                snapshot.reference,
                display_name(&snapshot.name)
            ),
            Self::Changed {
                reference,
                name,
                fields,
            } => {
                write!(f, "~ {reference:#x} {}", display_name(name))?;
                for change in fields {
                    write!(f, " {}: {} -> {}", change.field, change.old, change.new)?;
                }
                Ok(())
            }
        }
    }
}

fn display_name(name: &str) -> &str {
    if name.is_empty() { "<unnamed>" } else { name }
}

/// Changes from `old` to `new`, both in reference order, in reference order.
pub fn diff_objects(old: &[ObjectSnapshot], new: &[ObjectSnapshot]) -> Vec<ObjectChange> {
    let before: BTreeMap<_, _> = old.iter().map(|s| (s.reference, s)).collect();
    let after: BTreeMap<_, _> = new.iter().map(|s| (s.reference, s)).collect();
    let mut references: Vec<_> = before.keys().chain(after.keys()).copied().collect();
    references.sort_unstable();
    references.dedup();
    references
        .into_iter()
        .filter_map(
            |reference| match (before.get(&reference), after.get(&reference)) {
                (None, Some(added)) => Some(ObjectChange::Added(Box::new((*added).clone()))),
                (Some(removed), None) => Some(ObjectChange::Removed(Box::new((*removed).clone()))),
                (Some(old), Some(new)) if old != new => Some(ObjectChange::Changed {
                    reference,
                    name: new.name.clone(),
                    fields: diff_fields(old, new),
                }),
                _ => None,
            },
        )
        .collect()
}

/// Field-level differences, in field order; a field present on only one
/// side (a critter block added or removed) shows `-` on the other.
fn diff_fields(old: &ObjectSnapshot, new: &ObjectSnapshot) -> Vec<FieldChange> {
    let old_fields = old.fields();
    let new_fields = new.fields();
    let lookup = |fields: &[(&'static str, String)], field: &str| {
        fields
            .iter()
            .find(|(name, _)| *name == field)
            .map_or_else(|| "-".to_string(), |(_, value)| value.clone())
    };
    let mut changes: Vec<FieldChange> = new_fields
        .iter()
        .map(|(field, value)| FieldChange {
            field,
            old: lookup(&old_fields, field),
            new: value.clone(),
        })
        .collect();
    changes.extend(
        old_fields
            .iter()
            .filter(|(field, _)| !new_fields.iter().any(|(other, _)| other == field))
            .map(|(field, value)| FieldChange {
                field,
                old: value.clone(),
                new: "-".into(),
            }),
    );
    changes.retain(|change| change.old != change.new);
    changes
}

/// Tracks filtered objects across ticks.
#[derive(Debug, Clone, Default)]
pub struct ObjectTracker {
    pub filter: ObjectFilter,
    previous: Option<Vec<ObjectSnapshot>>,
}
impl ObjectTracker {
    pub fn new(filter: ObjectFilter) -> Self {
        Self {
            filter,
            previous: None,
        }
    }
    /// Snapshot the matching objects and return what changed since the
    /// previous tick. The first tick (and the first after `reset`) only
    /// records a baseline and reports nothing.
    pub fn tick(&mut self, world: &WorldState) -> Vec<ObjectChange> {
        let current = snapshot_objects(world, &self.filter);
        let changes = self
            .previous
            .as_deref()
            .map(|previous| diff_objects(previous, &current))
            .unwrap_or_default();
        self.previous = Some(current);
        changes
    }
    pub fn current(&self) -> Option<&[ObjectSnapshot]> {
        self.previous.as_deref()
    }
    pub fn reset(&mut self) {
        self.previous = None;
    }
}

/// A fixed-column object table, one row per object.
pub fn render_objects(objects: &[ObjectSnapshot]) -> String {
    let mut text = format!(
        "{:<8} {:>6} {:<10} {:<8} {:>5} {:>2} {:>2} {:>5} {:<10} {:<10} {:>6} {:<3} {}\n",
        "REF", "ID", "PID", "CAT", "HEX", "EL", "OR", "FRAME", "ART", "FLAGS", "SID", "ACT", "NAME"
    );
    for o in objects {
        text.push_str(&format!(
            "{:<8} {:>6} {:<10} {:<8} {:>5} {:>2} {:>2} {:>5} {:<10} {:<10} {:>6} {:<3} {}\n",
            format!("{:#x}", o.reference),
            o.id,
            format!("{:#010x}", o.pid),
            category_name(o.category),
            optional(o.hex),
            o.elevation,
            o.orientation,
            o.frame,
            format!("{:#010x}", o.art_id),
            format!("{:#010x}", o.flags),
            o.script_id,
            if o.active { "yes" } else { "no" },
            o.name
        ));
    }
    text
}

/// A fixed-column critter table for the objects that carry critter state.
pub fn render_critters(objects: &[ObjectSnapshot]) -> String {
    let mut text = format!(
        "{:<8} {:>4} {:>3} {:>4} {:>3} {:>4} {:>6} {:>4} {:<5} {:<6} {}\n",
        "REF", "HP", "AP", "TEAM", "AI", "CID", "POISON", "RAD", "ALIVE", "ROLE", "NAME"
    );
    for o in objects {
        let Some(c) = &o.critter else { continue };
        let role = if c.player {
            "player"
        } else if c.party {
            "party"
        } else {
            "-"
        };
        text.push_str(&format!(
            "{:<8} {:>4} {:>3} {:>4} {:>3} {:>4} {:>6} {:>4} {:<5} {:<6} {}\n",
            format!("{:#x}", o.reference),
            c.hp,
            c.ap,
            c.team,
            c.ai,
            c.combat_id,
            c.poison,
            c.radiation,
            if c.alive { "yes" } else { "no" },
            role,
            o.name
        ));
    }
    text
}

/// Every field of one object, `name: value` per line.
pub fn render_object_detail(object: &ObjectSnapshot) -> String {
    let mut text = format!("object {:#x}\n", object.reference);
    for (field, value) in object.fields() {
        text.push_str(&format!("  {field}: {value}\n"));
    }
    text
}
