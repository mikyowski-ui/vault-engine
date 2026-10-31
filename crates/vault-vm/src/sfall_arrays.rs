//! SPEC-024b part 2: sfall's dynamic arrays. The store belongs to the world,
//! not to a program, so a string in it is its own copy of the bytes.
//!
//! Everything a script can see follows the element order (§5.6): a list runs
//! by index, and a map in first-insertion order until an order operation
//! rewrites it. Nothing here depends on a hash map, an address or a locale.
//! A map is searched in that order: scripts' maps are small, and an index
//! would add state that the order must not depend on.
use crate::sfall::SfallValue;
use std::{cmp::Ordering, collections::BTreeMap};

/// `load_array`'s reserved key (§5.5).
pub const ALL_ARRAYS: &[u8] = b"...all_arrays...";
/// The sidecar's and the extension stream's tags (§5.8, §5.9).
pub const SIDECAR_TAG: &[u8; 4] = b"VSFL";
pub const STREAM_TAG: &[u8; 4] = b"VSX1";
const FORMAT_REVISION: u32 = 2;
/// The revision the engine no longer writes, whose header stops at offset
/// 12. It still loads (Decision D-32).
const OLD_REVISION: u32 = 1;

/// A list holds values; a map holds key and value pairs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Elements {
    List(Vec<SfallValue>),
    Map {
        lookup: bool,
        pairs: Vec<(SfallValue, SfallValue)>,
    },
}
/// §5.3 how long an array lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    Temporary = 0,
    Permanent = 1,
    Saved = 2,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfallArray {
    pub elements: Elements,
    pub storage: Storage,
    pub save_key: Option<SfallValue>,
}
impl SfallArray {
    fn len(&self) -> usize {
        match &self.elements {
            Elements::List(values) => values.len(),
            Elements::Map { pairs, .. } => pairs.len(),
        }
    }
}

fn number(value: &SfallValue) -> f64 {
    match value {
        SfallValue::Int(value) => f64::from(*value),
        value => f64::from(value.number()),
    }
}
/// §5.6's total order: numbers first, then strings. Numbers compare by
/// value, equal values put the int first, and NaN sorts after every other
/// number. Strings compare as unsigned bytes.
pub fn value_order(a: &SfallValue, b: &SfallValue) -> Ordering {
    use SfallValue::{Float, Int, Text};
    match (a, b) {
        (Text(a), Text(b)) => a.cmp(b),
        (Text(_), _) => Ordering::Greater,
        (_, Text(_)) => Ordering::Less,
        _ => {
            let (left, right) = (number(a), number(b));
            match (left.is_nan(), right.is_nan()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                _ => left.total_cmp(&right).then_with(|| match (a, b) {
                    (Int(_), Float(_)) => Ordering::Less,
                    (Float(_), Int(_)) => Ordering::Greater,
                    _ => Ordering::Equal,
                }),
            }
        }
    }
}
/// A list index from a key: a float truncates toward zero, and a string has
/// none.
fn list_index(key: &SfallValue, length: usize) -> Option<usize> {
    let index = match key {
        SfallValue::Int(index) => i64::from(*index),
        SfallValue::Float(bits) => f32::from_bits(*bits) as i64,
        SfallValue::Text(_) => return None,
    };
    usize::try_from(index).ok().filter(|index| *index < length)
}

/// The array store of one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayStore {
    arrays: BTreeMap<u32, SfallArray>,
    next_id: u32,
    /// The array `arrayexpr` assigns to. Not saved, and a load clears it.
    last_created: Option<u32>,
}
impl Default for ArrayStore {
    fn default() -> Self {
        Self {
            arrays: BTreeMap::new(),
            next_id: 1,
            last_created: None,
        }
    }
}
impl ArrayStore {
    pub fn arrays(&self) -> &BTreeMap<u32, SfallArray> {
        &self.arrays
    }
    pub fn next_id(&self) -> u32 {
        self.next_id
    }
    /// `true` while `id` names an array.
    pub fn exists(&self, id: u32) -> bool {
        self.arrays.contains_key(&id)
    }
    fn insert(&mut self, elements: Elements, storage: Storage) -> i32 {
        let id = self.next_id;
        self.next_id += 1;
        self.arrays.insert(
            id,
            SfallArray {
                elements,
                storage,
                save_key: None,
            },
        );
        self.last_created = Some(id);
        id as i32
    }
    /// `create_array` or `temp_array` (§5.4): `size >= 0` makes a list of
    /// that many int 0 values, whatever `flags` says; `size == -1` makes a
    /// map, a lookup map when `flags == 2`. Anything else makes no array.
    pub fn create(&mut self, size: i32, flags: i32, storage: Storage) -> i32 {
        let elements = match size {
            -1 => Elements::Map {
                lookup: flags == 2,
                pairs: Vec::new(),
            },
            size if size >= 0 => Elements::List(vec![SfallValue::Int(0); size as usize]),
            _ => return 0,
        };
        self.insert(elements, storage)
    }
    /// A new temporary list of `values`.
    pub fn temp_list(&mut self, values: Vec<SfallValue>) -> i32 {
        self.insert(Elements::List(values), Storage::Temporary)
    }
    /// A new map of `pairs`, in the order given.
    pub fn map_of(&mut self, pairs: Vec<(SfallValue, SfallValue)>, storage: Storage) -> i32 {
        let lookup = false;
        self.insert(Elements::Map { lookup, pairs }, storage)
    }
    fn array(&self, id: i32) -> Option<&SfallArray> {
        self.arrays.get(&u32::try_from(id).ok()?)
    }
    fn array_mut(&mut self, id: i32) -> Option<&mut SfallArray> {
        self.arrays.get_mut(&u32::try_from(id).ok()?)
    }
    /// `fix_array`: a temporary array becomes permanent.
    pub fn fix(&mut self, id: i32) {
        if let Some(array) = self.array_mut(id)
            && array.storage == Storage::Temporary
        {
            array.storage = Storage::Permanent;
        }
    }
    /// `set_array` (§5.4's assignment tables).
    pub fn set(&mut self, id: i32, key: SfallValue, value: SfallValue) {
        let Some(array) = self.array_mut(id) else {
            return;
        };
        match &mut array.elements {
            Elements::List(values) => {
                if let Some(index) = list_index(&key, values.len()) {
                    values[index] = value;
                }
            }
            Elements::Map { lookup, pairs } => {
                let found = pairs.iter().position(|(other, _)| *other == key);
                match (found, *lookup, value == SfallValue::Int(0)) {
                    (Some(_), true, _) | (None, false, true) => {}
                    (None, _, _) => pairs.push((key, value)),
                    (Some(at), false, true) => {
                        pairs.remove(at);
                    }
                    (Some(at), false, false) => pairs[at].1 = value,
                }
            }
        }
    }
    /// `arrayexpr`: an assignment to the last created array.
    pub fn expression(&mut self, key: SfallValue, value: SfallValue) {
        if let Some(id) = self.last_created {
            self.set(id as i32, key, value);
        }
    }
    /// `get_array`: the value, or int 0 for an absent key, an index out of
    /// range or a bad ID.
    pub fn get(&self, id: i32, key: &SfallValue) -> SfallValue {
        let found = self.array(id).and_then(|array| match &array.elements {
            Elements::List(values) => list_index(key, values.len()).map(|index| &values[index]),
            Elements::Map { pairs, .. } => pairs
                .iter()
                .find(|(other, _)| other == key)
                .map(|(_, value)| value),
        });
        found.cloned().unwrap_or(SfallValue::Int(0))
    }
    /// `len_array`: elements or pairs, or -1 for a bad ID.
    pub fn len(&self, id: i32) -> i32 {
        self.array(id).map_or(-1, |array| array.len() as i32)
    }
    /// Whether the store holds no array.
    pub fn is_empty(&self) -> bool {
        self.arrays.is_empty()
    }
    /// `free_array`: gone, and out of the save set with it.
    pub fn free(&mut self, id: i32) {
        if let Ok(id) = u32::try_from(id) {
            self.arrays.remove(&id);
        }
    }
    /// `scan_array`: the first match's list index or map key, else int -1.
    pub fn scan(&self, id: i32, value: &SfallValue) -> SfallValue {
        let found = self.array(id).and_then(|array| match &array.elements {
            Elements::List(values) => values
                .iter()
                .position(|other| other == value)
                .map(|index| SfallValue::Int(index as i32)),
            Elements::Map { pairs, .. } => pairs
                .iter()
                .find(|(_, other)| other == value)
                .map(|(key, _)| key.clone()),
        });
        found.unwrap_or(SfallValue::Int(-1))
    }
    /// `array_key`: with index -1 the shape; a list gives the index back
    /// unchecked, and a map the key at that position, or int 0.
    pub fn key_at(&self, id: i32, index: i32) -> SfallValue {
        let Some(array) = self.array(id) else {
            return SfallValue::Int(0);
        };
        match (&array.elements, index) {
            (Elements::List(_), -1) => SfallValue::Int(0),
            (Elements::Map { .. }, -1) => SfallValue::Int(1),
            (Elements::List(_), index) => SfallValue::Int(index),
            (Elements::Map { pairs, .. }, index) => usize::try_from(index)
                .ok()
                .and_then(|index| pairs.get(index))
                .map_or(SfallValue::Int(0), |(key, _)| key.clone()),
        }
    }
    /// `resize_array` (§5.4). `false` for a size that names no operation,
    /// and `draw(i)` gives a uniform integer in 0 to i for a shuffle.
    pub fn resize<E>(
        &mut self,
        id: i32,
        size: i32,
        mut draw: impl FnMut(i32) -> Result<i32, E>,
    ) -> Result<bool, E> {
        let Some(array) = self.array_mut(id) else {
            return Ok(true);
        };
        let length = array.len();
        match (size, &mut array.elements) {
            (0.., elements) => {
                let size = size as usize;
                match elements {
                    Elements::List(values) if size > length => {
                        values.resize(size, SfallValue::Int(0));
                    }
                    Elements::List(values) => values.truncate(size),
                    Elements::Map { pairs, .. } => pairs.truncate(size),
                }
            }
            (-2 | -6, Elements::List(values)) => values.sort_by(value_order),
            (-3 | -7, Elements::List(values)) => values.sort_by(|a, b| value_order(b, a)),
            (-2, Elements::Map { pairs, .. }) => pairs.sort_by(|a, b| value_order(&a.0, &b.0)),
            (-3, Elements::Map { pairs, .. }) => pairs.sort_by(|a, b| value_order(&b.0, &a.0)),
            (-6, Elements::Map { pairs, .. }) => pairs.sort_by(|a, b| value_order(&a.1, &b.1)),
            (-7, Elements::Map { pairs, .. }) => pairs.sort_by(|a, b| value_order(&b.1, &a.1)),
            (-4, Elements::List(values)) => values.reverse(),
            (-4, Elements::Map { pairs, .. }) => pairs.reverse(),
            (-5, elements) => {
                for at in (1..length).rev() {
                    let other = usize::try_from(draw(at as i32)?).unwrap_or(0).min(at);
                    match elements {
                        Elements::List(values) => values.swap(at, other),
                        Elements::Map { pairs, .. } => pairs.swap(at, other),
                    }
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    /// `save_array` (§5.5): any key but int 0 makes the array saved under
    /// it, taking it from any array that held it; int 0 unsaves.
    pub fn save(&mut self, key: SfallValue, id: i32) {
        if self.array(id).is_none() {
            return;
        }
        let unsave = key == SfallValue::Int(0);
        if !unsave {
            for other in self.arrays.values_mut() {
                if other.save_key.as_ref() == Some(&key) {
                    other.save_key = None;
                    other.storage = Storage::Permanent;
                }
            }
        }
        let array = self.array_mut(id).expect("the array exists");
        if unsave {
            if array.storage == Storage::Saved {
                array.storage = Storage::Permanent;
                array.save_key = None;
            }
        } else {
            array.storage = Storage::Saved;
            array.save_key = Some(key);
        }
    }
    /// `load_array`: the ID holding `key`, or 0; the reserved key gives a
    /// new temporary list of all saved keys, in §5.6's order.
    pub fn load(&mut self, key: &SfallValue) -> i32 {
        if *key == SfallValue::text(ALL_ARRAYS) {
            let mut keys: Vec<SfallValue> = self
                .arrays
                .values()
                .filter_map(|array| array.save_key.clone())
                .collect();
            keys.sort_by(value_order);
            return self.temp_list(keys);
        }
        self.arrays
            .iter()
            .find(|(_, array)| array.save_key.as_ref() == Some(key))
            .map_or(0, |(id, _)| *id as i32)
    }
    /// The end of a frame's Simulation phase (§5.3): temporaries go.
    pub fn end_of_step(&mut self) {
        self.arrays
            .retain(|_, array| array.storage != Storage::Temporary);
    }
}

fn write_value(out: &mut Vec<u8>, value: &SfallValue) {
    out.push(value.type_code() as u8);
    match value {
        SfallValue::Int(value) => out.extend(value.to_le_bytes()),
        SfallValue::Float(bits) => out.extend(bits.to_le_bytes()),
        SfallValue::Text(text) => {
            out.extend((text.len() as u32).to_le_bytes());
            out.extend(text);
        }
    }
}
/// The fields format revision 2 adds to the header (§5.8). The default is
/// what a new game has, and what a revision 1 sidecar migrates to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub next_unique: u32,
    /// -1 for no pending forced encounter.
    pub forced_map: i32,
    pub forced_flags: u32,
}
impl Default for Header {
    fn default() -> Self {
        Self {
            next_unique: 0x1000_0000,
            forced_map: -1,
            forced_flags: 0,
        }
    }
}
/// §5.8's layout under `tag`, with `globals` in the order given: the sidecar
/// holds the saved arrays only, and the extension stream every live array.
pub fn write_stream(
    tag: &[u8; 4],
    store: &ArrayStore,
    header: Header,
    globals: &[(SfallValue, SfallValue)],
    saved_only: bool,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(tag);
    out.extend(FORMAT_REVISION.to_le_bytes());
    out.extend(store.next_id.to_le_bytes());
    out.extend(header.next_unique.to_le_bytes());
    out.extend(header.forced_map.to_le_bytes());
    out.extend(header.forced_flags.to_le_bytes());
    out.extend((globals.len() as u32).to_le_bytes());
    for (key, value) in globals {
        write_value(&mut out, key);
        write_value(&mut out, value);
    }
    let arrays: Vec<(&u32, &SfallArray)> = store
        .arrays
        .iter()
        .filter(|(_, array)| !saved_only || array.storage == Storage::Saved)
        .collect();
    out.extend((arrays.len() as u32).to_le_bytes());
    for (id, array) in arrays {
        out.extend(id.to_le_bytes());
        let (shape, lookup) = match &array.elements {
            Elements::List(_) => (0, false),
            Elements::Map { lookup, .. } => (1, *lookup),
        };
        out.extend([shape, u8::from(lookup), array.storage as u8, 0]);
        match &array.save_key {
            Some(key) => {
                out.push(1);
                write_value(&mut out, key);
            }
            None => out.push(0),
        }
        out.extend((array.len() as u32).to_le_bytes());
        match &array.elements {
            Elements::List(values) => {
                for value in values {
                    write_value(&mut out, value);
                }
            }
            Elements::Map { pairs, .. } => {
                for (key, value) in pairs {
                    write_value(&mut out, key);
                    write_value(&mut out, value);
                }
            }
        }
    }
    out
}

/// Why a sidecar was refused (§5.8): saved data is never dropped silently.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SidecarError {
    #[error("the tag is not VSFL")]
    Tag,
    #[error("format revision {0} is unknown")]
    Revision(u32),
    #[error("the file ends early")]
    Truncated,
    #[error("{0} bytes follow the last record")]
    Trailing(usize),
    #[error("type byte {0} is unknown")]
    Type(u8),
    #[error("array {0} appears twice")]
    DuplicateId(u32),
    #[error("shape byte {0} is outside 0-1")]
    Shape(u8),
    #[error("storage byte {0} is outside 0-2")]
    Storage(u8),
    #[error("option byte {0} is outside 0-1")]
    Option(u8),
    #[error("{0} is not a sfall global's key")]
    GlobalKey(String),
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], SidecarError> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(SidecarError::Truncated)?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }
    fn u8(&mut self) -> Result<u8, SidecarError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, SidecarError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().expect("four bytes")))
    }
    fn value(&mut self) -> Result<SfallValue, SidecarError> {
        match self.u8()? {
            1 => Ok(SfallValue::Int(self.u32()? as i32)),
            2 => Ok(SfallValue::Float(self.u32()?)),
            3 => {
                let length = self.u32()? as usize;
                Ok(SfallValue::Text(self.take(length)?.to_vec()))
            }
            other => Err(SidecarError::Type(other)),
        }
    }
}
/// A parsed sidecar: the store as saved, its header's fields, and the global
/// records in order.
pub type Sidecar = (ArrayStore, Header, Vec<(SfallValue, SfallValue)>);
/// Parse a whole `VAULTSFL.DAT`, before any state changes (§5.8). The tag
/// and the revision come first, and only then is a layout chosen. A revision
/// 1 file has no fields of [`Header`], so they take their defaults.
pub fn parse_sidecar(bytes: &[u8]) -> Result<Sidecar, SidecarError> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(4).map_err(|_| SidecarError::Tag)? != SIDECAR_TAG {
        return Err(SidecarError::Tag);
    }
    let revision = reader.u32()?;
    if revision != FORMAT_REVISION && revision != OLD_REVISION {
        return Err(SidecarError::Revision(revision));
    }
    let next_id = reader.u32()?;
    let header = if revision == FORMAT_REVISION {
        Header {
            next_unique: reader.u32()?,
            forced_map: reader.u32()? as i32,
            forced_flags: reader.u32()?,
        }
    } else {
        Header::default()
    };
    let mut globals = Vec::new();
    for _ in 0..reader.u32()? {
        let key = reader.value()?;
        globals.push((key, reader.value()?));
    }
    let mut arrays = BTreeMap::new();
    for _ in 0..reader.u32()? {
        let id = reader.u32()?;
        let shape = reader.u8()?;
        let lookup = reader.u8()? != 0;
        let storage = match reader.u8()? {
            0 => Storage::Temporary,
            1 => Storage::Permanent,
            2 => Storage::Saved,
            other => return Err(SidecarError::Storage(other)),
        };
        reader.u8()?;
        let save_key = match reader.u8()? {
            0 => None,
            1 => Some(reader.value()?),
            other => return Err(SidecarError::Option(other)),
        };
        let count = reader.u32()?;
        let elements = match shape {
            0 => {
                let mut values = Vec::new();
                for _ in 0..count {
                    values.push(reader.value()?);
                }
                Elements::List(values)
            }
            1 => {
                let mut pairs = Vec::new();
                for _ in 0..count {
                    let key = reader.value()?;
                    pairs.push((key, reader.value()?));
                }
                Elements::Map { lookup, pairs }
            }
            other => return Err(SidecarError::Shape(other)),
        };
        let array = SfallArray {
            elements,
            storage,
            save_key,
        };
        if arrays.insert(id, array).is_some() {
            return Err(SidecarError::DuplicateId(id));
        }
    }
    let left = bytes.len() - reader.at;
    if left != 0 {
        return Err(SidecarError::Trailing(left));
    }
    let store = ArrayStore {
        arrays,
        next_id,
        last_created: None,
    };
    Ok((store, header, globals))
}
