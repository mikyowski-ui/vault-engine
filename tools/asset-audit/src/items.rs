//! SPEC-011 §4.12 structural checks of the item prototypes and `perk.msg`
//! that the drug, armor, weapon, ammunition, and money rules depend on.
//! Counts are reported next to the specification's survey; only the
//! invariants decide success.
use std::{collections::BTreeMap, error::Error, sync::Arc};
use vault_core::{
    CharacterSheet, WorldEffect, WorldHost, WorldState,
    assets::{VfsWorldAssets, WorldAssets},
    drugs::{ADDICTION_TABLE, CURRENT_HP, RADIATION, RANGE_FORM, Stage},
    queue::EventKind,
};
use vault_formats::{
    LocalDirProvider, MAX_ASSET_BYTES, MsgCatalog, Prototype, Vfs,
    pro::{ExtendedFlags, Item, ItemData, PrototypeData},
};

/// Item counts by subtype (armor through key) in the §4.12 survey.
pub const SURVEY_SUBTYPES: [usize; 7] = [9, 77, 14, 43, 16, 77, 6];
const SUBTYPES: [&str; 7] = [
    "armor",
    "container",
    "drug",
    "weapon",
    "ammunition",
    "misc",
    "key",
];

/// Every §4.12 invariant, in report order. Each one is listed in the report
/// even when this corpus has nothing it applies to, so an invariant can
/// never drop out of the table (or the pass count) silently.
const INVARIANTS: [&str; 18] = [
    "item files parse with subtype 0-6",
    "item file length equals the SPEC-003 reader size",
    "drug selectors are -1, 0-37, or -2 in slot 0",
    "range-form drugs heal through slot 1",
    "no delayed stage pairs a delay with all-zero amounts",
    "addiction chances are 0-100",
    "addictive drugs name withdrawal perks 53-57",
    "addiction-table PIDs are drugs",
    "alcohol PIDs 124 and 125 are not addictive",
    "healing PIDs 40 and 144 are drugs selecting current HP",
    "armor perks are -1, 62, or 63",
    "armor DR is 0-90 (EMP above 100)",
    "armor DT is 0-100",
    "magazine weapons have matching ammunition",
    "weapon minimum strength is 0-10",
    "ammunition pack sizes are above 0",
    "money PID 41 costs 1",
    "perk.msg names 101-164 and descriptions 201-264",
];

/// One invariant: how many subjects it was checked against, and the
/// violations among them.
#[derive(Debug)]
pub struct Invariant {
    pub name: &'static str,
    pub checked: usize,
    pub violations: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ItemAudit {
    pub subtypes: [usize; 7],
    pub range_form: usize,
    pub delayed: usize,
    pub two_handed: usize,
    pub magazines: usize,
    /// Every entry of [`INVARIANTS`], passing at zero violations.
    pub invariants: Vec<Invariant>,
}
impl ItemAudit {
    pub fn failed(&self) -> usize {
        self.invariants
            .iter()
            .filter(|invariant| !invariant.violations.is_empty())
            .count()
    }
    /// Invariants this corpus gives nothing to check (reported, not failed).
    pub fn unexercised(&self) -> usize {
        self.invariants
            .iter()
            .filter(|invariant| invariant.checked == 0)
            .count()
    }
    /// The survey counts this corpus reproduces.
    pub fn survey(&self) -> [(&'static str, usize, usize); 5] {
        [
            ("item prototypes", self.subtypes.iter().sum(), 242),
            ("range-form drugs", self.range_form, 2),
            ("scheduled delayed stages", self.delayed, 18),
            ("two-handed weapons", self.two_handed, 16),
            ("weapons with magazines", self.magazines, 26),
        ]
    }
    pub fn report(&self) {
        let counts: Vec<_> = SUBTYPES
            .iter()
            .zip(self.subtypes)
            .map(|(name, count)| format!("{name} {count}"))
            .collect();
        println!(
            "Item prototypes: {} ({})",
            self.subtypes.iter().sum::<usize>(),
            counts.join(", ")
        );
        for (name, found, survey) in self.survey() {
            let note = if found == survey {
                "matches the §4.12 survey".to_string()
            } else {
                format!("§4.12 survey reported {survey}")
            };
            println!("{name}: {found} ({note})");
        }
        if self.subtypes != SURVEY_SUBTYPES {
            println!("subtype counts differ from the §4.12 survey {SURVEY_SUBTYPES:?}");
        }
        println!("invariant\tchecked\tviolations");
        for invariant in &self.invariants {
            println!(
                "{}\t{}\t{}",
                invariant.name,
                invariant.checked,
                invariant.violations.len()
            );
            if invariant.checked == 0 {
                println!(
                    "NOT EXERCISED {}: nothing in this corpus to check",
                    invariant.name
                );
            }
            for violation in invariant.violations.iter().take(5) {
                eprintln!("FAIL {}: {violation}", invariant.name);
            }
        }
        println!(
            "Item invariants: {} passed, {} failed, {} not exercised",
            self.invariants.len() - self.failed() - self.unexercised(),
            self.failed(),
            self.unexercised()
        );
    }
}

struct Checks(Vec<Invariant>);
impl Checks {
    fn new() -> Self {
        Self(
            INVARIANTS
                .iter()
                .map(|name| Invariant {
                    name,
                    checked: 0,
                    violations: Vec::new(),
                })
                .collect(),
        )
    }
    fn check(&mut self, name: &'static str, ok: bool, violation: impl FnOnce() -> String) {
        let invariant = self
            .0
            .iter_mut()
            .find(|invariant| invariant.name == name)
            .expect("every checked invariant is listed in INVARIANTS");
        invariant.checked += 1;
        if !ok {
            invariant.violations.push(violation());
        }
    }
}

pub fn audit_items(vfs: &dyn Vfs) -> Result<ItemAudit, Box<dyn Error>> {
    let mut audit = ItemAudit::default();
    let mut checks = Checks::new();
    let mut items: BTreeMap<u32, Item> = BTreeMap::new();
    for path in vfs.files() {
        if path.extension() != Some("pro") || !path.as_str().starts_with("proto/items/") {
            continue;
        }
        let bytes = vfs.read_bounded(path.as_str(), MAX_ASSET_BYTES)?;
        let parsed = Prototype::parse(&bytes);
        checks.check("item files parse with subtype 0-6", parsed.is_ok(), || {
            format!(
                "{path}: {}",
                parsed
                    .as_ref()
                    .err()
                    .map_or(String::new(), |e| e.to_string())
            )
        });
        let Ok(prototype) = parsed else {
            continue;
        };
        checks.check(
            "item file length equals the SPEC-003 reader size",
            prototype.consumed_len == bytes.len(),
            || {
                format!(
                    "{path}: {} of {} bytes read",
                    prototype.consumed_len,
                    bytes.len()
                )
            },
        );
        let PrototypeData::Item(item) = prototype.data else {
            checks.check("item files parse with subtype 0-6", false, || {
                format!("{path}: not an item prototype")
            });
            continue;
        };
        audit.subtypes[item.data.subtype() as usize] += 1;
        if let ItemData::Weapon(_) = item.data
            && prototype.extended.contains(ExtendedFlags::TWO_HANDED)
        {
            audit.two_handed += 1;
        }
        items.insert(prototype.pid, item);
    }
    let calibers: Vec<_> = items
        .values()
        .filter_map(|item| match item.data {
            ItemData::Ammunition { caliber, .. } => Some(caliber),
            _ => None,
        })
        .collect();
    for (pid, item) in &items {
        match &item.data {
            ItemData::Drug(drug) => {
                for (slot, selector) in drug.stats.iter().enumerate() {
                    let valid = (-1..=RADIATION).contains(selector)
                        || (slot == 0 && *selector == RANGE_FORM);
                    checks.check(
                        "drug selectors are -1, 0-37, or -2 in slot 0",
                        valid,
                        || format!("PID {pid}: slot {slot} selector {selector}"),
                    );
                }
                if drug.stats[0] == RANGE_FORM {
                    audit.range_form += 1;
                    checks.check(
                        "range-form drugs heal through slot 1",
                        drug.stats[1] == CURRENT_HP,
                        || format!("PID {pid}: slot 1 selector {}", drug.stats[1]),
                    );
                }
                for (delay, amounts) in [(drug.delay1, drug.changes1), (drug.delay2, drug.changes2)]
                {
                    let scheduled = amounts.iter().any(|amount| *amount != 0);
                    audit.delayed += usize::from(scheduled);
                    checks.check(
                        "no delayed stage pairs a delay with all-zero amounts",
                        scheduled || delay == 0,
                        || format!("PID {pid}: delay {delay} with zero amounts"),
                    );
                }
                checks.check(
                    "addiction chances are 0-100",
                    (0..=100).contains(&drug.addiction),
                    || format!("PID {pid}: {}%", drug.addiction),
                );
                checks.check(
                    "addictive drugs name withdrawal perks 53-57",
                    drug.addiction == 0 || (53..=57).contains(&drug.withdrawal),
                    || format!("PID {pid}: perk {}", drug.withdrawal),
                );
            }
            ItemData::Armor(armour) => {
                checks.check(
                    "armor perks are -1, 62, or 63",
                    matches!(armour.perk, -1 | 62 | 63),
                    || format!("PID {pid}: perk {}", armour.perk),
                );
                for (kind, resistance) in armour.resistances.iter().enumerate() {
                    let valid = if kind == 5 {
                        *resistance > 100
                    } else {
                        (0..=90).contains(resistance)
                    };
                    checks.check("armor DR is 0-90 (EMP above 100)", valid, || {
                        format!("PID {pid}: damage type {kind} DR {resistance}")
                    });
                }
                checks.check(
                    "armor DT is 0-100",
                    armour
                        .thresholds
                        .iter()
                        .all(|value| (0..=100).contains(value)),
                    || format!("PID {pid}: DT {:?}", armour.thresholds),
                );
            }
            ItemData::Weapon(weapon) => {
                if weapon.capacity > 0 {
                    audit.magazines += 1;
                    checks.check(
                        "magazine weapons have matching ammunition",
                        calibers.contains(&weapon.caliber),
                        || format!("PID {pid}: caliber {}", weapon.caliber),
                    );
                }
                checks.check(
                    "weapon minimum strength is 0-10",
                    (0..=10).contains(&weapon.strength),
                    || format!("PID {pid}: strength {}", weapon.strength),
                );
            }
            ItemData::Ammunition { quantity, .. } => {
                checks.check("ammunition pack sizes are above 0", *quantity > 0, || {
                    format!("PID {pid}: pack {quantity}")
                })
            }
            _ => {}
        }
    }
    let drug = |pid: u32| match items.get(&pid).map(|item| &item.data) {
        Some(ItemData::Drug(drug)) => Some(drug.clone()),
        _ => None,
    };
    for (pid, _) in ADDICTION_TABLE {
        checks.check(
            "addiction-table PIDs are drugs",
            drug(pid).is_some(),
            || format!("PID {pid}"),
        );
    }
    for pid in [124, 125] {
        checks.check(
            "alcohol PIDs 124 and 125 are not addictive",
            drug(pid).is_some_and(|drug| drug.addiction == 0),
            || format!("PID {pid}"),
        );
    }
    for pid in [vault_core::ai::STIMPAK, vault_core::ai::SUPER_STIMPAK] {
        checks.check(
            "healing PIDs 40 and 144 are drugs selecting current HP",
            drug(pid).is_some_and(|drug| drug.stats.contains(&CURRENT_HP)),
            || format!("PID {pid}"),
        );
    }
    checks.check(
        "money PID 41 costs 1",
        items
            .get(&vault_core::items::MONEY_PID)
            .is_some_and(|item| item.cost == 1),
        || "PID 41".into(),
    );
    let perks = vfs
        .read_bounded("text/english/game/perk.msg", MAX_ASSET_BYTES)
        .map_err(|error| error.to_string())
        .and_then(|bytes| MsgCatalog::parse(&bytes).map_err(|error| error.to_string()));
    match perks {
        Ok(catalog) => {
            for id in (101..=164).chain(201..=264) {
                checks.check(
                    "perk.msg names 101-164 and descriptions 201-264",
                    catalog.get(id).is_some(),
                    || format!("message {id}"),
                );
            }
        }
        Err(error) => checks.check(
            "perk.msg names 101-164 and descriptions 201-264",
            false,
            || error,
        ),
    }
    audit.invariants = checks.0;
    Ok(audit)
}

/// Engine smoke check over the corpus drugs: the player (at 1 HP, with the
/// installed campaign variables and message files) consumes each drug.
/// Every consumption must return 1, queue exactly the drug's nonzero
/// delayed stages, and show at least one stage message; the current-HP
/// drugs must heal. Returns drugs consumed, delayed stages queued, healing
/// drugs, and failures.
pub fn consume_drugs(vfs: &Arc<LocalDirProvider>) -> Result<[usize; 4], Box<dyn Error>> {
    let mut pids = Vec::new();
    for path in vfs.files() {
        if path.extension() != Some("pro") || !path.as_str().starts_with("proto/items/") {
            continue;
        }
        // Unreadable item files are the invariant audit's failures.
        let Ok(prototype) = vfs
            .read_bounded(path.as_str(), MAX_ASSET_BYTES)
            .map_err(|error| error.to_string())
            .and_then(|bytes| Prototype::parse(&bytes).map_err(|error| error.to_string()))
        else {
            continue;
        };
        if let PrototypeData::Item(Item {
            data: ItemData::Drug(drug),
            ..
        }) = &prototype.data
        {
            pids.push((prototype.pid, drug.clone()));
        }
    }
    let [mut consumed, mut queued, mut healed, mut failures] = [0; 4];
    for (pid, drug) in pids {
        let assets = VfsWorldAssets::new(vfs.clone(), "english")?;
        let mut host = WorldHost::new(WorldState::new(CharacterSheet::default(), 1)?, assets);
        host.load_campaign()?;
        let player = host.world.player;
        host.world.set_hit_points(player, 1);
        // One drug that cannot be spawned or consumed is a failure; the
        // remaining drugs are still consumed.
        let consumed_by = |host: &mut WorldHost<VfsWorldAssets>| -> Result<i32, Box<dyn Error>> {
            let item = host.world.spawn(host.assets.prototype(pid)?, 0, 0)?;
            host.world.effects.clear();
            Ok(host.consume_drug(player, item)?)
        };
        let result = match consumed_by(&mut host) {
            Ok(result) => result,
            Err(error) => {
                failures += 1;
                eprintln!("FAIL drug consumption PID {pid}: {error}");
                continue;
            }
        };
        let stages = Stage::delayed(&drug)
            .iter()
            .filter(|(_, stage)| stage.scheduled())
            .count();
        let drug_events = host
            .world
            .queue
            .events()
            .filter(|event| matches!(event.kind, EventKind::Drug { .. }))
            .count();
        let shown = host
            .world
            .effects
            .iter()
            .any(|effect| matches!(effect, WorldEffect::Message(_)));
        let heals = drug.stats.contains(&CURRENT_HP);
        let hp = host.world.hit_points(player).unwrap_or(0);
        if result != 1 || drug_events != stages || !shown || (heals && hp <= 1) {
            failures += 1;
            eprintln!(
                "FAIL drug consumption PID {pid}: result {result}, {drug_events} of {stages} stages, \
                 message shown {shown}, HP {hp}"
            );
        }
        consumed += 1;
        queued += drug_events;
        healed += usize::from(heals && hp > 1);
    }
    Ok([consumed, queued, healed, failures])
}
