//! SPEC-024c §3: the `ddraw.ini` keys that change how a session starts. The
//! file is read once, under sfall mode only, when the session is built; a
//! replay reads it as any INI input. With no file every key takes its
//! default, with no diagnostic. A value of the wrong form takes its default
//! with one diagnostic, and never stops the session.
use std::collections::BTreeMap;
use vault_formats::fallout2::text::{self, Diagnostics, Section, SectionedText};

pub const FILE: &str = "ddraw.ini";
/// §3.3's defaults: 25 July 2241, by 0-indexed month and day.
const DEFAULT_DATE: (u16, usize, u64) = (2241, 6, 24);
/// SPEC-018 §6.1's non-leap calendar.
const MONTH_DAYS: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
/// §3.3 `VersionString`'s and §3.10's limits.
const MAX_VERSION: usize = 63;
const MAX_PREMADE_NAME: usize = 11;

/// §3.9 `MainMenuFontColour`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuColour {
    pub index: u8,
    /// Flag 1: the colour reaches the version text only.
    pub version_only: bool,
}
/// §3.8 the player models, by gender: male, then female.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerModels {
    pub start: [Option<String>; 2],
    pub default: [Option<String>; 2],
}
/// What a key's text read as.
enum Reading<T> {
    /// Absent, empty, or -1: the default, with no diagnostic.
    Unset,
    /// The default, with a diagnostic already written.
    Invalid,
    Value(T),
}

/// The session's startup overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupOptions {
    /// §3.4 the stem, lowercase, not yet tested against `maps/`.
    pub starting_map: Option<String>,
    /// §3.5 each index's file under `art/cuts/`.
    pub movies: BTreeMap<usize, String>,
    /// §3.6 the reset tick.
    pub start_ticks: Option<u64>,
    /// §3.6 each key alone, by 0-indexed month and day.
    year: Option<u16>,
    month: Option<usize>,
    day: Option<u64>,
    /// §3.7 world pixels, not yet tested against the world.
    pub world_pos: Option<[i32; 2]>,
    pub view_pos: Option<[i32; 2]>,
    pub models: PlayerModels,
    /// §3.9 with its numbers filled in.
    pub version: Option<Vec<u8>>,
    pub menu_offset: [i32; 2],
    pub credits_offset: [i32; 2],
    pub menu_colour: Option<MenuColour>,
    /// §3.10 each premade's name and FID, in order.
    pub premades: Option<Vec<(String, i32)>>,
    /// `EnableHeroAppearanceMod` (§2.4).
    pub hero_appearance: i32,
    /// The diagnostics of reading the file.
    pub notes: Vec<String>,
}

/// A new game's start, once the tables have loaded (SPEC-018 §6.6 step 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGameStart {
    /// `maps/<stem>.map`.
    pub map: String,
    pub world_pos: Option<[i32; 2]>,
    pub notes: Vec<String>,
}

impl StartupOptions {
    /// `bytes`, in SPEC-018 §3.3's grammar. Only `[Misc]` counts.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut options = Self::default();
        let mut notes = Diagnostics::default();
        let table = match SectionedText::parse(FILE, bytes, &mut notes) {
            Ok(table) => table,
            Err(error) => {
                options.notes.push(error.to_string());
                return options;
            }
        };
        options
            .notes
            .extend(notes.0.iter().map(ToString::to_string));
        if let Some(misc) = table.section("Misc") {
            options.read(misc);
        }
        options
    }
    fn invalid(&mut self, key: &str, value: &str) {
        self.notes.push(format!("{FILE}: {key}={value} is invalid"));
    }
    fn text<'a>(section: &'a Section, key: &str) -> Option<&'a str> {
        section
            .get(key)
            .map(|value| value.text.as_str())
            .filter(|text| !text.is_empty())
    }
    /// A numeric key: -1 is the published "leave as default".
    fn number(&mut self, section: &Section, key: &str) -> Reading<i32> {
        let Some(value) = Self::text(section, key) else {
            return Reading::Unset;
        };
        match text::integer(value) {
            Some(-1) => Reading::Unset,
            Some(number) => Reading::Value(number),
            None => {
                self.invalid(key, value);
                Reading::Invalid
            }
        }
    }
    /// A numeric key that must meet `valid`.
    fn number_if(
        &mut self,
        section: &Section,
        key: &str,
        valid: impl Fn(i32) -> bool,
    ) -> Option<i32> {
        match self.number(section, key) {
            Reading::Value(number) if valid(number) => Some(number),
            Reading::Value(number) => {
                self.invalid(key, &number.to_string());
                None
            }
            _ => None,
        }
    }
    /// Two keys that apply as a pair: one alone is invalid (§3.7).
    fn pair(&mut self, section: &Section, keys: [&str; 2]) -> Option<[i32; 2]> {
        match (self.number(section, keys[0]), self.number(section, keys[1])) {
            (Reading::Value(x), Reading::Value(y)) => Some([x, y]),
            (Reading::Unset, Reading::Unset) | (Reading::Invalid, _) | (_, Reading::Invalid) => {
                None
            }
            _ => {
                self.notes.push(format!(
                    "{FILE}: {} and {} apply as a pair",
                    keys[0], keys[1]
                ));
                None
            }
        }
    }
    fn offset(&mut self, section: &Section, keys: [&str; 2]) -> [i32; 2] {
        keys.map(|key| match self.number(section, key) {
            Reading::Value(number) => number,
            _ => 0,
        })
    }
    fn read(&mut self, misc: &Section) {
        self.read_map(misc);
        self.read_movies(misc);
        self.read_time(misc);
        self.world_pos = self.pair(misc, ["StartXPos", "StartYPos"]);
        self.view_pos = self.pair(misc, ["ViewXPos", "ViewYPos"]);
        let name = |key: &str| Self::text(misc, key).map(str::to_owned);
        self.models = PlayerModels {
            start: [name("MaleStartModel"), name("FemaleStartModel")],
            default: [name("MaleDefaultModel"), name("FemaleDefaultModel")],
        };
        self.read_title(misc);
        self.read_premades(misc);
        if let Reading::Value(mode) = self.number(misc, "EnableHeroAppearanceMod") {
            self.hero_appearance = mode;
        }
    }
    /// §3.4: a Stem, so the final `.map` is optional.
    fn read_map(&mut self, misc: &Section) {
        let Some(value) = Self::text(misc, "StartingMap") else {
            return;
        };
        match text::stem(value) {
            Some(stem) => self.starting_map = Some(stem.to_ascii_lowercase()),
            None => self.invalid("StartingMap", value),
        }
    }
    /// §3.5: index `N - 1`. The published `Movie18` to `Movie32` have no
    /// slot in the 17-byte played record.
    fn read_movies(&mut self, misc: &Section) {
        for number in 1..=32 {
            let key = format!("Movie{number}");
            let Some(value) = Self::text(misc, &key) else {
                continue;
            };
            if number > 17 {
                self.notes.push(format!("{FILE}: {key} is not supported"));
                continue;
            }
            let lower = value.to_ascii_lowercase();
            let stem = lower
                .strip_suffix(".mve")
                .filter(|stem| !stem.ends_with(".map") && text::stem(stem).is_some());
            match stem {
                Some(_) => {
                    self.movies.insert(number - 1, lower);
                }
                None => self.invalid(&key, value),
            }
        }
    }
    /// §3.6: `HHMM`, and the date of game day 0, each key alone.
    fn read_time(&mut self, misc: &Section) {
        let time = self.number_if(misc, "StartTime", |value| {
            (0..).contains(&value) && value / 100 <= 23 && value % 100 <= 59
        });
        self.start_ticks = time.map(|value| {
            let (hours, minutes) = (u64::from(value as u32 / 100), u64::from(value as u32 % 100));
            (60 * hours + minutes) * 600
        });
        let year = self.number_if(misc, "StartYear", |value| {
            (0..=i32::from(u16::MAX)).contains(&value)
        });
        self.year = year.map(|year| year as u16);
        let month = self.number_if(misc, "StartMonth", |value| (0..12).contains(&value));
        self.month = month.map(|month| month as usize);
        let days = MONTH_DAYS[self.month.unwrap_or(DEFAULT_DATE.1)] as i32;
        let day = self.number_if(misc, "StartDay", |value| (0..days).contains(&value));
        self.day = day.map(|day| day as u64);
    }
    /// §3.9.
    fn read_title(&mut self, misc: &Section) {
        if let Some(value) = Self::text(misc, "VersionString") {
            match version_text(value) {
                Some(version) => self.version = Some(version),
                None => self.invalid("VersionString", value),
            }
        }
        self.menu_offset = self.offset(misc, ["MainMenuOffsetX", "MainMenuOffsetY"]);
        self.credits_offset =
            self.offset(misc, ["MainMenuCreditsOffsetX", "MainMenuCreditsOffsetY"]);
        let Some(value) = Self::text(misc, "MainMenuFontColour") else {
            return;
        };
        if text::integer(value) == Some(-1) {
            return;
        }
        let digits = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .unwrap_or(value);
        let Some(colour) = u32::from_str_radix(digits, 16)
            .ok()
            .filter(|c| *c <= 0xff_ffff)
        else {
            self.invalid("MainMenuFontColour", value);
            return;
        };
        let flags = (colour >> 16) & 0xff;
        if flags & 6 != 0 {
            self.notes.push(format!(
                "{FILE}: MainMenuFontColour flags {flags} are not supported"
            ));
        }
        self.menu_colour = Some(MenuColour {
            index: (colour & 0xff) as u8,
            version_only: flags & 1 != 0,
        });
    }
    /// §3.10: both set, with equal counts, and short names.
    fn read_premades(&mut self, misc: &Section) {
        let (paths, fids) = (
            Self::text(misc, "PremadePaths"),
            Self::text(misc, "PremadeFIDs"),
        );
        if paths.is_none() && fids.is_none() {
            return;
        }
        let parsed = paths.zip(fids).and_then(|(paths, fids)| {
            let names = text::list(paths);
            let fids: Option<Vec<i32>> =
                text::list(fids).iter().map(|f| text::integer(f)).collect();
            let fids = fids.filter(|fids| fids.len() == names.len())?;
            let short = |name: &String| (1..=MAX_PREMADE_NAME).contains(&name.len());
            names
                .iter()
                .all(short)
                .then(|| names.into_iter().zip(fids).collect())
        });
        match parsed {
            Some(premades) => self.premades = Some(premades),
            None => self.notes.push(format!(
                "{FILE}: PremadePaths and PremadeFIDs are two lists of one length"
            )),
        }
    }
    /// §3.6 the year, and the day of that year (from 0), of game day 0.
    pub fn calendar(&self) -> (u16, u64) {
        let year = self.year.unwrap_or(DEFAULT_DATE.0);
        let month = self.month.unwrap_or(DEFAULT_DATE.1);
        let day = self.day.unwrap_or(DEFAULT_DATE.2);
        (year, MONTH_DAYS[..month].iter().sum::<u64>() + day)
    }
    /// §3.5 index `index`'s file, by default SPEC-018 §6.4's.
    pub fn movie(&self, index: usize) -> Option<&str> {
        self.movies.get(&index).map(String::as_str)
    }
    /// SPEC-018 §6.6 step 1: the checks that need the files and the world.
    /// Nothing has changed yet, so a bad value falls back cleanly.
    pub fn new_game_start(
        &self,
        default_map: &str,
        exists: impl Fn(&str) -> bool,
        world: [i32; 2],
    ) -> NewGameStart {
        let mut notes = Vec::new();
        let mut map = default_map.to_owned();
        if let Some(stem) = &self.starting_map {
            let path = format!("maps/{stem}.map");
            if exists(&path) {
                map = path;
            } else {
                notes.push(format!("{FILE}: StartingMap={stem}: no {path}"));
            }
        }
        let inside = |[x, y]: &[i32; 2]| (0..world[0]).contains(x) && (0..world[1]).contains(y);
        let world_pos = self.world_pos.filter(inside);
        if let Some([x, y]) = self.world_pos.filter(|point| !inside(point)) {
            notes.push(format!(
                "{FILE}: StartXPos,StartYPos {x},{y} is outside the world"
            ));
        }
        NewGameStart {
            map,
            world_pos,
            notes,
        }
    }
    /// §3.8: the models `critters.lst` holds, with a diagnostic for each it
    /// does not.
    pub fn checked_models(&self, holds: impl Fn(&str) -> bool) -> (PlayerModels, Vec<String>) {
        let mut notes = Vec::new();
        let mut check = |key: &str, name: &Option<String>| {
            let name = name.as_ref()?;
            if holds(name) {
                return Some(name.clone());
            }
            notes.push(format!("{FILE}: {key}={name} is not in critters.lst"));
            None
        };
        let models = &self.models;
        let checked = PlayerModels {
            start: [
                check("MaleStartModel", &models.start[0]),
                check("FemaleStartModel", &models.start[1]),
            ],
            default: [
                check("MaleDefaultModel", &models.default[0]),
                check("FemaleDefaultModel", &models.default[1]),
            ],
        };
        (checked, notes)
    }
}

/// §3.9: at most 63 bytes, and up to two `%d`, which become 1 and 2. Any
/// other `%` is invalid.
fn version_text(value: &str) -> Option<Vec<u8>> {
    let bytes = text::to_bytes(value);
    if bytes.len() > MAX_VERSION {
        return None;
    }
    let mut out = Vec::new();
    let mut rest = bytes.as_slice();
    let mut number = 0;
    while let Some((first, tail)) = rest.split_first() {
        if *first != b'%' {
            out.push(*first);
            rest = tail;
            continue;
        }
        rest = tail.strip_prefix(b"d")?;
        number += 1;
        if number > 2 {
            return None;
        }
        out.extend(number.to_string().as_bytes());
    }
    Some(out)
}
