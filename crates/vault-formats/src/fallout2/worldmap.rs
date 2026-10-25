//! SPEC-018 §4.4 `data/worldmap.txt`: terrains and frequencies, the tile
//! grid and its subtiles, walk masks, random maps, encounter tables and
//! groups, and the condition grammar; §5.3's grid geometry.
use super::{
    maps::MapsTable,
    text::{self, Diagnostics, Section, SectionedText, TableError},
};
use std::collections::BTreeMap;

pub const FILE: &str = "data/worldmap.txt";
/// A tile is 350 by 300 world pixels of 7 by 6 subtiles.
pub const TILE_SIZE: [i32; 2] = [350, 300];
pub const SUBTILES: [usize; 2] = [7, 6];
pub const SUBTILE: i32 = 50;
pub const MAX_TILE: u32 = 255;
pub const MAX_ACROSS: i32 = 16;
/// §4.4.2 a walk mask: 300 rows of 44 bytes.
pub const MASK_ROW: usize = 44;
pub const MASK_LEN: usize = 300 * MASK_ROW;

/// One terrain type: its name and travel difficulty 1-10.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terrain {
    pub name: String,
    pub difficulty: i32,
}

/// §4.4.2 item 2: the reveal direction of a subtile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fill {
    #[default]
    None,
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Fill {
    fn parse(text: &str) -> Option<Self> {
        Some(match text::fold(text).as_str() {
            "no_fill" => Self::None,
            "fill_n" => Self::N,
            "fill_s" => Self::S,
            "fill_e" => Self::E,
            "fill_w" => Self::W,
            "fill_ne" => Self::NE,
            "fill_nw" => Self::NW,
            "fill_se" => Self::SE,
            "fill_sw" => Self::SW,
            _ => return None,
        })
    }
    /// §5.5 the column and row steps it fills toward: `[dx, dy]` pairs.
    pub fn directions(self) -> &'static [[i32; 2]] {
        match self {
            Self::None => &[],
            Self::N => &[[0, -1]],
            Self::S => &[[0, 1]],
            Self::E => &[[1, 0]],
            Self::W => &[[-1, 0]],
            Self::NE => &[[0, -1], [1, 0]],
            Self::NW => &[[0, -1], [-1, 0]],
            Self::SE => &[[0, 1], [1, 0]],
            Self::SW => &[[0, 1], [-1, 0]],
        }
    }
}

/// One subtile: the Fallout 2 world cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Subtile {
    /// Index in [`WorldmapTable::terrains`].
    pub terrain: usize,
    pub fill: Fill,
    /// Encounter percentages: morning, afternoon, night.
    pub frequency: [i32; 3],
    /// The encounter table's number.
    pub table: Option<u32>,
}

/// One `[Tile N]` (or a default tile below `T` without a section).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub art: i32,
    pub encounter_difficulty: i32,
    pub walk_mask: Option<String>,
    /// `[y][x]`.
    pub subtiles: [[Subtile; 7]; 6],
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            art: -1,
            encounter_difficulty: 0,
            walk_mask: None,
            subtiles: [[Subtile::default(); 7]; 6],
        }
    }
}

/// §4.4.4 a group expression's relation words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    And,
    Fighting,
    /// Always followed by `Player`.
    Ambush,
}

/// `(n)` or `(a-b)` of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupCount {
    pub min: i32,
    pub max: i32,
    /// The `[Encounter: <name>]` group, folded.
    pub name: String,
}

/// An entry's `Enc:` expression: its groups in order, and the relation
/// before each later target (a group, or `Player` after `AMBUSH`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Expression {
    pub groups: Vec<GroupCount>,
    pub relations: Vec<Relation>,
}

/// §4.4.6 a comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// §4.4.6 an operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    Global(i32),
    PlayerLevel,
    DaysPlayed,
    TimeOfDay,
}

/// One clause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clause {
    Compare(Operand, Compare, i32),
    Rand(i32),
}

/// A condition: always true, permanently false (it did not parse), or
/// clauses joined left to right (`true` is `And`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Condition {
    #[default]
    Always,
    Never,
    Clauses {
        first: Clause,
        rest: Vec<(bool, Clause)>,
    },
}

/// What a condition reads (§4.4.6).
pub trait ConditionContext {
    fn global(&self, index: i32) -> i32;
    fn player_level(&self) -> i32;
    fn ticks(&self) -> u64;
    /// SPEC-005 §3.5's HHMM.
    fn time_of_day(&self) -> i32;
    /// `random(1, 100)` from the shared generator.
    fn percent_draw(&mut self) -> i32;
}

impl Condition {
    /// §4.4.6 evaluation: clause by clause, left to right, no precedence and
    /// no short-circuit, so every `Rand` draws; a condition that did not
    /// parse is false and draws nothing.
    pub fn holds(&self, context: &mut dyn ConditionContext) -> bool {
        let clause = |clause: &Clause, context: &mut dyn ConditionContext| match *clause {
            Clause::Rand(p) => context.percent_draw() <= p,
            Clause::Compare(operand, compare, value) => {
                let left = match operand {
                    Operand::Global(index) => i64::from(context.global(index)),
                    Operand::PlayerLevel => i64::from(context.player_level()),
                    Operand::DaysPlayed => (context.ticks() / 864000) as i64,
                    Operand::TimeOfDay => i64::from(context.time_of_day()),
                };
                let value = i64::from(value);
                match compare {
                    Compare::Eq => left == value,
                    Compare::Ne => left != value,
                    Compare::Lt => left < value,
                    Compare::Le => left <= value,
                    Compare::Gt => left > value,
                    Compare::Ge => left >= value,
                }
            }
        };
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Clauses { first, rest } => {
                let mut value = clause(first, context);
                for (and, next) in rest {
                    let next = clause(next, context);
                    value = if *and { value && next } else { value || next };
                }
                value
            }
        }
    }
}

/// One `enc_NN` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub number: u32,
    pub chance: i32,
    /// -1 is unlimited.
    pub counter: i32,
    pub special: bool,
    pub map: Option<u32>,
    pub expression: Expression,
    pub condition: Condition,
}

/// One `[Encounter Table N]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncounterTable {
    pub number: u32,
    pub lookup_name: String,
    pub maps: Vec<u32>,
    /// In ascending entry number.
    pub entries: Vec<Entry>,
}

/// §4.4.5 a group's formation (stored for SPEC-019).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Formation {
    #[default]
    Surrounding,
    StraightLine,
    DoubleLine,
    Wedge,
    Cone,
    Huddle,
}

/// An `Item:` stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemSpec {
    pub min: i32,
    pub max: i32,
    pub pid: u32,
    pub wielded: bool,
}

/// One `type_NN` critter type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CritterType {
    pub number: u32,
    pub pid: u32,
    pub ratio: Option<i32>,
    pub dead: bool,
    /// Zero-based `scripts.lst` index; -1 uses the prototype's script.
    pub script: i32,
    pub items: Vec<ItemSpec>,
    pub condition: Condition,
}

/// One `[Encounter: <name>]` group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub formation: Formation,
    pub spacing: i32,
    pub distance: i32,
    /// In ascending type number.
    pub types: Vec<CritterType>,
}

/// A loaded walk mask (§4.4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkMask(Vec<u8>);

impl WalkMask {
    /// Exactly 13200 bytes.
    pub fn parse(bytes: Vec<u8>) -> Option<Self> {
        (bytes.len() == MASK_LEN).then_some(Self(bytes))
    }
    /// Whether tile-local pixel `(x, y)` blocks travel; x of 350 or more
    /// never does.
    pub fn blocks(&self, [x, y]: [i32; 2]) -> bool {
        if !(0..TILE_SIZE[0]).contains(&x) || !(0..TILE_SIZE[1]).contains(&y) {
            return false;
        }
        let byte = self.0[y as usize * MASK_ROW + x as usize / 8];
        byte & (0x80 >> (x % 8)) != 0
    }
}

/// The parsed world-map table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldmapTable {
    pub terrains: Vec<Terrain>,
    /// Frequency names (folded) to percentages.
    pub frequencies: BTreeMap<String, i32>,
    /// `H`, tiles across.
    pub across: i32,
    /// `T` tiles: index = tile number.
    pub tiles: Vec<Tile>,
    /// Each terrain's random map list.
    pub random_maps: Vec<Vec<u32>>,
    pub tables: BTreeMap<u32, EncounterTable>,
    /// By folded name.
    pub groups: BTreeMap<String, Group>,
}

impl WorldmapTable {
    pub fn parse(
        bytes: &[u8],
        maps: &MapsTable,
        notes: &mut Diagnostics,
    ) -> Result<Self, TableError> {
        let file = FILE;
        let table = SectionedText::parse(file, bytes, notes)?;
        // §4.4.1 [Data].
        let data = table.section("data");
        let mut terrains = Vec::new();
        if let Some(value) = data.and_then(|data| data.get("terrain_types")) {
            for item in text::list(&value.text) {
                let parsed = item.split_once(':').and_then(|(name, difficulty)| {
                    Some(Terrain {
                        name: text::name(name.trim())?,
                        difficulty: text::integer_in(difficulty.trim(), 1..=10)?,
                    })
                });
                match parsed {
                    Some(terrain) => terrains.push(terrain),
                    None => notes.push(file, value.line, format!("terrain {item:?} dropped")),
                }
            }
        }
        if terrains.is_empty() {
            terrains = [("Desert", 1), ("Mountain", 2), ("City", 1), ("Ocean", 1)]
                .map(|(name, difficulty)| Terrain {
                    name: name.into(),
                    difficulty,
                })
                .into();
        }
        let mut frequencies: BTreeMap<String, i32> = [
            ("forced", 100),
            ("frequent", 38),
            ("common", 22),
            ("uncommon", 12),
            ("rare", 4),
            ("none", 0),
        ]
        .map(|(name, value)| (name.to_owned(), value))
        .into();
        if let Some(data) = data {
            for (key, value) in data.pairs() {
                if key == "terrain_types" {
                    continue;
                }
                if let Some(percent) = text::percent(&value.text) {
                    frequencies.insert(key.to_owned(), percent);
                }
            }
        }
        // §4.4.5 groups, then §4.4.4 tables, which subtiles name.
        let mut groups = BTreeMap::new();
        for (name, section) in table.with_prefix("encounter:") {
            let group = group(&name, section, notes, file);
            groups.insert(name, group);
        }
        let mut tables: BTreeMap<u32, EncounterTable> = BTreeMap::new();
        for (number, section) in table.numbered("encounter table") {
            if number > 999 {
                notes.push(
                    file,
                    section.line,
                    format!("encounter table {number} outside 0-999"),
                );
                continue;
            }
            let Some(lookup_name) = section
                .get("lookup_name")
                .and_then(|value| text::name(&value.text))
            else {
                notes.push(
                    file,
                    section.line,
                    format!("[{}] has no lookup_name; dropped", section.name),
                );
                continue;
            };
            let mut table_maps = Vec::new();
            if let Some(value) = section.get("maps") {
                for item in text::list(&value.text) {
                    match maps.by_lookup_name(&item) {
                        Some(map) => table_maps.push(map),
                        None => notes.push(file, value.line, format!("maps: unknown map {item:?}")),
                    }
                }
            }
            let mut entries = Vec::new();
            for (n, value) in section.indexed("enc", 0..=99) {
                match entry(n, &value.text, maps, &groups, notes, file, value.line) {
                    Some(entry) => entries.push(entry),
                    None => notes.push(file, value.line, format!("enc_{n:02} dropped")),
                }
            }
            tables.insert(
                number,
                EncounterTable {
                    number,
                    lookup_name,
                    maps: table_maps,
                    entries,
                },
            );
        }
        let mut names: BTreeMap<String, u32> = BTreeMap::new();
        for table in tables.values() {
            let key = text::fold(&table.lookup_name);
            if let Some(first) = names.get(&key) {
                notes.push(
                    file,
                    0,
                    format!(
                        "encounter table {} repeats table {first}'s name",
                        table.number
                    ),
                );
            } else {
                names.insert(key, table.number);
            }
        }
        // §4.4.2 tiles.
        let across = table
            .section("tile data")
            .and_then(|section| section.get("num_horizontal_tiles"))
            .and_then(|value| text::integer_in(&value.text, 1..=MAX_ACROSS))
            .ok_or_else(|| {
                TableError::new(file, 0, "[Tile Data] num_horizontal_tiles must be 1-16")
            })?;
        let numbered: Vec<(u32, &Section)> = table
            .numbered("tile")
            .into_iter()
            .filter(|(number, section)| {
                let ok = *number <= MAX_TILE;
                if !ok {
                    notes.push(file, section.line, format!("tile {number} outside 0-255"));
                }
                ok
            })
            .collect();
        let Some(highest) = numbered.iter().map(|(number, _)| *number).max() else {
            return Err(TableError::new(file, 0, "no valid [Tile N]"));
        };
        let mut tiles = vec![Tile::default(); highest as usize + 1];
        let terrain_index = |name: &str| {
            terrains
                .iter()
                .position(|terrain| terrain.name.eq_ignore_ascii_case(name))
        };
        for (number, section) in numbered {
            let tile = &mut tiles[number as usize];
            if let Some(value) = section.get("art_idx") {
                tile.art = text::integer_in(&value.text, -1..=i32::MAX).unwrap_or_else(|| {
                    notes.push(file, value.line, "art_idx: invalid; -1");
                    -1
                });
            }
            if let Some(value) = section.get("encounter_difficulty") {
                tile.encounter_difficulty = text::integer(&value.text).unwrap_or_else(|| {
                    notes.push(file, value.line, "encounter_difficulty: invalid; 0");
                    0
                });
            }
            if let Some(value) = section.get("walk_mask_name")
                && !value.text.is_empty()
            {
                tile.walk_mask = text::stem(&value.text);
                if tile.walk_mask.is_none() {
                    notes.push(file, value.line, "walk_mask_name: not a Stem; none");
                }
            }
            for y in 0..SUBTILES[1] {
                for x in 0..SUBTILES[0] {
                    let key = format!("{x}_{y}");
                    let Some(value) = section.get(&key) else {
                        notes.push(
                            file,
                            section.line,
                            format!("[{}] has no {key}; defaults", section.name),
                        );
                        continue;
                    };
                    let items = text::list(&value.text);
                    if !(5..=6).contains(&items.len()) {
                        notes.push(
                            file,
                            value.line,
                            format!("{key}: {} items; defaults", items.len()),
                        );
                        continue;
                    }
                    let subtile = &mut tile.subtiles[y][x];
                    subtile.terrain = terrain_index(&items[0]).unwrap_or_else(|| {
                        notes.push(
                            file,
                            value.line,
                            format!("{key}: unknown terrain {:?}", items[0]),
                        );
                        0
                    });
                    subtile.fill = Fill::parse(&items[1]).unwrap_or_else(|| {
                        notes.push(
                            file,
                            value.line,
                            format!("{key}: unknown fill {:?}", items[1]),
                        );
                        Fill::None
                    });
                    for period in 0..3 {
                        let name = &items[2 + period];
                        subtile.frequency[period] = frequencies
                            .get(&text::fold(name))
                            .copied()
                            .unwrap_or_else(|| {
                                notes.push(
                                    file,
                                    value.line,
                                    format!("{key}: unknown frequency {name:?}"),
                                );
                                0
                            });
                    }
                    subtile.table = items.get(5).and_then(|name| {
                        let found = names.get(&text::fold(name)).copied();
                        if found.is_none() {
                            notes.push(
                                file,
                                value.line,
                                format!("{key}: unknown encounter table {name:?}"),
                            );
                        }
                        found
                    });
                }
            }
        }
        // §4.4.3 random maps.
        let mut random_maps = vec![Vec::new(); terrains.len()];
        for (name, section) in table.with_prefix("random maps:") {
            let Some(terrain) = terrain_index(&name) else {
                notes.push(
                    file,
                    section.line,
                    format!("[{}]: unknown terrain", section.name),
                );
                continue;
            };
            for (n, value) in section.indexed("map", 0..=99) {
                match maps.by_lookup_name(&value.text) {
                    Some(map) => random_maps[terrain].push(map),
                    None => notes.push(
                        file,
                        value.line,
                        format!("map_{n:02}: unknown map {:?}", value.text),
                    ),
                }
            }
        }
        Ok(Self {
            terrains,
            frequencies,
            across,
            tiles,
            random_maps,
            tables,
            groups,
        })
    }
    /// `C = 7 x H` grid columns and `R = 6 x ceil(T / H)` rows.
    pub fn grid(&self) -> [usize; 2] {
        let across = self.across as usize;
        [
            SUBTILES[0] * across,
            SUBTILES[1] * self.tiles.len().div_ceil(across),
        ]
    }
    /// The world's size in pixels: `50 x C` by `50 x R`.
    pub fn world_size(&self) -> [i32; 2] {
        let [columns, rows] = self.grid();
        [SUBTILE * columns as i32, SUBTILE * rows as i32]
    }
    /// The grid cell holding world pixel `position`, if inside the world.
    pub fn cell_of(&self, position: [i32; 2]) -> Option<[usize; 2]> {
        let [columns, rows] = self.grid();
        let [c, r] = position.map(|value| value.div_euclid(SUBTILE));
        ((0..columns as i32).contains(&c) && (0..rows as i32).contains(&r))
            .then_some([c as usize, r as usize])
    }
    /// The tile number of cell `[c, r]`.
    pub fn tile_of(&self, [c, r]: [usize; 2]) -> usize {
        (r / SUBTILES[1]) * self.across as usize + c / SUBTILES[0]
    }
    /// §5.3 cell `[c, r]`'s subtile; a tile at or beyond `T` takes the
    /// defaults.
    pub fn subtile(&self, cell: [usize; 2]) -> Subtile {
        self.tiles
            .get(self.tile_of(cell))
            .map_or_else(Subtile::default, |tile| {
                tile.subtiles[cell[1] % SUBTILES[1]][cell[0] % SUBTILES[0]]
            })
    }
    /// The difficulty of a terrain index (1 for an unknown one).
    pub fn difficulty(&self, terrain: usize) -> i32 {
        self.terrains.get(terrain).map_or(1, |t| t.difficulty)
    }
    /// The walk mask stems the tiles name.
    pub fn mask_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .tiles
            .iter()
            .filter_map(|t| t.walk_mask.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }
    /// The group named `name` (folded).
    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.get(&text::fold(name))
    }
}

fn group(name: &str, section: &Section, notes: &mut Diagnostics, file: &str) -> Group {
    let (mut formation, mut spacing, mut distance) = (Formation::default(), 1, 8);
    if let Some(value) = section.get("position") {
        let items = text::list(&value.text);
        if let Some(first) = items.first() {
            formation = match text::fold(first).as_str() {
                "surrounding" => Formation::Surrounding,
                "straight_line" => Formation::StraightLine,
                "double_line" => Formation::DoubleLine,
                "wedge" => Formation::Wedge,
                "cone" => Formation::Cone,
                "huddle" => Formation::Huddle,
                _ => {
                    notes.push(
                        file,
                        value.line,
                        format!("position: unknown formation {first:?}"),
                    );
                    Formation::default()
                }
            };
        }
        for item in items.iter().skip(1) {
            match text::field(item) {
                Some((keyword, value)) if keyword == "spacing" => {
                    spacing = text::integer_in(value, 1..=10).unwrap_or(1)
                }
                Some((keyword, value)) if keyword == "distance" => {
                    distance = text::integer_in(value, 0..=30).unwrap_or(8)
                }
                _ => notes.push(file, section.line, format!("position: {item:?} ignored")),
            }
        }
    }
    let mut types = Vec::new();
    for (n, value) in section.indexed("type", 0..=99) {
        match critter_type(n, &value.text, notes, file, value.line) {
            Some(critter) => types.push(critter),
            None => notes.push(
                file,
                value.line,
                format!("[{}] type_{n:02} dropped", section.name),
            ),
        }
    }
    Group {
        name: name.to_owned(),
        formation,
        spacing,
        distance,
        types,
    }
}

/// `If(...)`'s inner text, when `field` is an `If` field.
fn if_field(field: &str) -> Option<&str> {
    let head = field.get(..3)?;
    if !head.eq_ignore_ascii_case("if(") || !field.ends_with(')') {
        return None;
    }
    Some(&field[3..field.len() - 1])
}

fn critter_type(
    number: u32,
    value: &str,
    notes: &mut Diagnostics,
    file: &str,
    line: usize,
) -> Option<CritterType> {
    let mut out = CritterType {
        number,
        pid: 0,
        ratio: None,
        dead: false,
        script: -1,
        items: Vec::new(),
        condition: Condition::Always,
    };
    let mut pid = None;
    for item in text::list(value) {
        if let Some(inner) = if_field(&item) {
            out.condition = condition(inner).unwrap_or_else(|| {
                notes.push(
                    file,
                    line,
                    format!("type_{number:02}: condition {inner:?} never holds"),
                );
                Condition::Never
            });
            continue;
        }
        if item.eq_ignore_ascii_case("dead") {
            out.dead = true;
            continue;
        }
        let Some((keyword, field)) = text::field(&item) else {
            notes.push(file, line, format!("type_{number:02}: {item:?} ignored"));
            continue;
        };
        match keyword.as_str() {
            "pid" => pid = Some(text::integer(field).filter(|pid| pid >> 24 == 1)? as u32),
            "ratio" => out.ratio = Some(text::percent(field)?),
            "script" => out.script = text::integer_in(field, -1..=i32::MAX)?,
            "item" => match item_spec(field) {
                Some(spec) => out.items.push(spec),
                None => notes.push(
                    file,
                    line,
                    format!("type_{number:02}: item {field:?} dropped"),
                ),
            },
            _ => notes.push(file, line, format!("type_{number:02}: {item:?} ignored")),
        }
    }
    out.pid = pid?;
    Some(out)
}

/// `[ "(" count [ "-" count ] ")" ] pid [ "(wielded)" ]`.
fn item_spec(text: &str) -> Option<ItemSpec> {
    let mut rest = text.trim();
    let (mut min, mut max) = (1, 1);
    if let Some(after) = rest.strip_prefix('(') {
        let (inside, tail) = after.split_once(')')?;
        (min, max) = count_range(inside, 0..=1000)?;
        rest = tail.trim();
    }
    let mut wielded = false;
    let lower = rest.to_ascii_lowercase();
    if let Some(head) = lower.strip_suffix("(wielded)") {
        wielded = true;
        rest = rest[..head.len()].trim();
    }
    let pid = text::integer(rest).filter(|pid| *pid >= 0 && pid >> 24 == 0)? as u32;
    Some(ItemSpec {
        min,
        max,
        pid,
        wielded,
    })
}

/// `n` or `a-b`, each in `range`, with a not above b.
fn count_range(text: &str, range: std::ops::RangeInclusive<i32>) -> Option<(i32, i32)> {
    let (min, max) = match text.split_once('-') {
        Some((a, b)) => (a.trim(), b.trim()),
        None => (text.trim(), text.trim()),
    };
    let min = text::integer_in(min, range.clone())?;
    let max = text::integer_in(max, range)?;
    (min <= max).then_some((min, max))
}

#[allow(clippy::too_many_arguments)]
fn entry(
    number: u32,
    value: &str,
    maps: &MapsTable,
    groups: &BTreeMap<String, Group>,
    notes: &mut Diagnostics,
    file: &str,
    line: usize,
) -> Option<Entry> {
    let mut out = Entry {
        number,
        chance: 0,
        counter: -1,
        special: false,
        map: None,
        expression: Expression::default(),
        condition: Condition::Always,
    };
    for item in text::list(value) {
        if let Some(inner) = if_field(&item) {
            out.condition = condition(inner).unwrap_or_else(|| {
                notes.push(
                    file,
                    line,
                    format!("enc_{number:02}: condition {inner:?} never holds"),
                );
                Condition::Never
            });
            continue;
        }
        if item.eq_ignore_ascii_case("special") {
            out.special = true;
            continue;
        }
        let Some((keyword, field)) = text::field(&item) else {
            notes.push(file, line, format!("enc_{number:02}: {item:?} ignored"));
            continue;
        };
        match keyword.as_str() {
            "chance" => out.chance = text::percent(field)?,
            "counter" => out.counter = text::integer_in(field, -1..=1000)?,
            "map" => out.map = Some(maps.by_lookup_name(field)?),
            "enc" => out.expression = expression(field)?,
            _ => notes.push(file, line, format!("enc_{number:02}: {item:?} ignored")),
        }
    }
    for group in &out.expression.groups {
        if !groups.contains_key(&group.name) {
            notes.push(
                file,
                line,
                format!("enc_{number:02}: unknown group {}", group.name),
            );
        }
    }
    Some(out)
}

/// §4.4.4 the group expression grammar.
fn expression(text: &str) -> Option<Expression> {
    let mut rest = text.trim_start();
    let mut out = Expression::default();
    let group = |rest: &mut &str| -> Option<GroupCount> {
        let after = rest.strip_prefix('(')?;
        let (inside, tail) = after.split_once(')')?;
        let (min, max) = count_range(inside, 0..=25)?;
        let tail = tail.trim_start();
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '(')
            .unwrap_or(tail.len());
        let name = super::text::name(&tail[..end])?;
        *rest = tail[end..].trim_start();
        Some(GroupCount {
            min,
            max,
            name: super::text::fold(&name),
        })
    };
    out.groups.push(group(&mut rest)?);
    while !rest.is_empty() {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = rest[..end].to_ascii_lowercase();
        rest = rest[end..].trim_start();
        let relation = match word.as_str() {
            "and" => Relation::And,
            "fighting" => Relation::Fighting,
            "ambush" => Relation::Ambush,
            _ => return None,
        };
        out.relations.push(relation);
        if rest.len() >= 6 && rest[..6].eq_ignore_ascii_case("player") {
            if relation != Relation::Ambush {
                return None;
            }
            rest = rest[6..].trim_start();
        } else {
            if relation == Relation::Ambush {
                return None;
            }
            out.groups.push(group(&mut rest)?);
        }
    }
    Some(out)
}

/// A condition token.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Number(i32),
    Open,
    Close,
    Percent,
    Op(Compare),
}

fn tokens(text: &str) -> Option<Vec<Token>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let c = bytes[at];
        match c {
            b' ' | b'\t' => at += 1,
            b'(' => {
                out.push(Token::Open);
                at += 1;
            }
            b')' => {
                out.push(Token::Close);
                at += 1;
            }
            b'%' => {
                out.push(Token::Percent);
                at += 1;
            }
            b'=' | b'!' | b'<' | b'>' => {
                let two = bytes.get(at + 1) == Some(&b'=');
                let op = match (c, two) {
                    (b'=', true) | (b'=', false) => Compare::Eq,
                    (b'!', true) => Compare::Ne,
                    (b'<', true) => Compare::Le,
                    (b'<', false) => Compare::Lt,
                    (b'>', true) => Compare::Ge,
                    (b'>', false) => Compare::Gt,
                    _ => return None,
                };
                out.push(Token::Op(op));
                at += 1 + usize::from(two);
            }
            b'+' | b'-' | b'0'..=b'9' => {
                let start = at;
                at += 1;
                while at < bytes.len() && bytes[at].is_ascii_digit() {
                    at += 1;
                }
                out.push(Token::Number(text::integer(&text[start..at])?));
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = at;
                while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                    at += 1;
                }
                out.push(Token::Word(text[start..at].to_ascii_lowercase()));
            }
            _ => return None,
        }
    }
    Some(out)
}

/// §4.4.6 the condition grammar; `None` when it does not match.
pub fn condition(text: &str) -> Option<Condition> {
    let tokens = tokens(text)?;
    let mut at = 0;
    let word = |at: usize| match tokens.get(at) {
        Some(Token::Word(word)) => Some(word.as_str()),
        _ => None,
    };
    let clause = |at: &mut usize| -> Option<Clause> {
        let start = *at;
        let operand = match word(start)? {
            "rand" => {
                if tokens.get(start + 1) != Some(&Token::Open) {
                    return None;
                }
                let Some(Token::Number(p)) = tokens.get(start + 2) else {
                    return None;
                };
                let mut next = start + 3;
                if tokens.get(next) == Some(&Token::Percent) {
                    next += 1;
                }
                if tokens.get(next) != Some(&Token::Close) || !(0..=100).contains(p) {
                    return None;
                }
                *at = next + 1;
                return Some(Clause::Rand(*p));
            }
            "global" => {
                if tokens.get(start + 1) != Some(&Token::Open) {
                    return None;
                }
                let Some(Token::Number(index)) = tokens.get(start + 2) else {
                    return None;
                };
                if tokens.get(start + 3) != Some(&Token::Close) {
                    return None;
                }
                *at = start + 4;
                Operand::Global(*index)
            }
            "player" => {
                if tokens.get(start + 1) != Some(&Token::Open)
                    || word(start + 2) != Some("level")
                    || tokens.get(start + 3) != Some(&Token::Close)
                {
                    return None;
                }
                *at = start + 4;
                Operand::PlayerLevel
            }
            "days_played" => {
                *at = start + 1;
                Operand::DaysPlayed
            }
            "time_of_day" => {
                *at = start + 1;
                Operand::TimeOfDay
            }
            _ => return None,
        };
        let Some(Token::Op(compare)) = tokens.get(*at) else {
            return None;
        };
        let Some(Token::Number(value)) = tokens.get(*at + 1) else {
            return None;
        };
        *at += 2;
        Some(Clause::Compare(operand, *compare, *value))
    };
    let first = clause(&mut at)?;
    let mut rest = Vec::new();
    while at < tokens.len() {
        let and = match word(at)? {
            "and" => true,
            "or" => false,
            _ => return None,
        };
        at += 1;
        rest.push((and, clause(&mut at)?));
    }
    Some(Condition::Clauses { first, rest })
}
