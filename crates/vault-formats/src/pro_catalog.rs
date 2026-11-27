use crate::{
    LstKind, LstTable, MAX_ASSET_BYTES, ParseError, Prototype, Vfs,
    mod_manifest::ProtoRegistration,
    pro::{Category, PrototypeInfo, PrototypeLookup},
};
use std::{cell::RefCell, collections::BTreeMap};

/// On-demand, cached subtype resolution through one-based prototype lists.
///
/// The resolved prototype list of a category is its list file followed by
/// the rows mod layers register (`docs/extended-art-namespaces.md` §3.4,
/// [`Self::with_registered`]): an entry number past the list file that a
/// registration names resolves to `proto/<directory>/<file>`.
pub struct LocalPrototypeCatalog<'a> {
    vfs: &'a dyn Vfs,
    game: crate::GameProfile,
    lists: RefCell<BTreeMap<Category, LstTable>>,
    prototypes: RefCell<BTreeMap<u32, PrototypeInfo>>,
    /// Registered file names by (category number, one-based entry number).
    registered: BTreeMap<(u8, u32), String>,
}
impl<'a> LocalPrototypeCatalog<'a> {
    /// A Fallout 1 catalog (SPEC-003 prototypes).
    pub fn new(vfs: &'a dyn Vfs) -> Self {
        Self::for_game(vfs, crate::GameProfile::Fallout1)
    }
    /// A catalog reading `game`'s prototype layouts (SPEC-016).
    pub fn for_game(vfs: &'a dyn Vfs, game: crate::GameProfile) -> Self {
        Self {
            vfs,
            game,
            lists: RefCell::new(BTreeMap::new()),
            prototypes: RefCell::new(BTreeMap::new()),
            registered: BTreeMap::new(),
        }
    }
    /// This catalog with registered prototype rows (`[[protos]]`): a PID
    /// whose entry number lies past its category's list file and equals a
    /// registered `index` reads `proto/<directory>/<file>`. Entry numbers
    /// the list file holds keep their list rows; the launch checks that
    /// refuse such a registration are the caller's. A later row for the same
    /// (category, index) replaces an earlier one.
    pub fn with_registered(mut self, rows: impl IntoIterator<Item = ProtoRegistration>) -> Self {
        for row in rows {
            self.registered.insert((row.category, row.index), row.file);
        }
        self
    }
    /// The registered file name of PID `pid`'s (category, entry number),
    /// whether or not the list file also holds that entry.
    pub fn registered_file(&self, pid: u32) -> Option<&str> {
        let category = Category::from_pid(pid).ok()? as u8;
        self.registered
            .get(&(category, pid & 0x00ff_ffff))
            .map(String::as_str)
    }
    fn read(&self, path: &str) -> Result<Vec<u8>, ParseError> {
        self.vfs
            .read_bounded(path, MAX_ASSET_BYTES)
            .map_err(|error| ParseError::new("PRO lookup", 0, error.to_string()))
    }
}
impl PrototypeLookup for LocalPrototypeCatalog<'_> {
    fn lookup(&self, pid: u32) -> Result<PrototypeInfo, ParseError> {
        if let Some(info) = self.prototypes.borrow().get(&pid) {
            return Ok(*info);
        }
        let category = Category::from_pid(pid)?;
        let folder = category.directory();
        if !self.lists.borrow().contains_key(&category) {
            let bytes = self.read(&format!("proto/{folder}/{folder}.lst"))?;
            self.lists
                .borrow_mut()
                .insert(category, LstTable::parse(&bytes, LstKind::Prototype)?);
        }
        let lists = self.lists.borrow();
        let list = &lists[&category];
        // Past the list file, a registered row answers the entry number.
        let entry = pid & 0x00ff_ffff;
        let past_list = usize::try_from(entry).is_ok_and(|entry| entry > list.entries().len());
        let registered = past_list
            .then(|| self.registered.get(&(category as u8, entry)))
            .flatten()
            .map(String::as_bytes);
        let name = registered
            .or_else(|| {
                list.prototype_id(pid)
                    .and_then(|entry| entry.name.as_deref())
            })
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                ParseError::new(
                    "PRO lookup",
                    0,
                    format!("PID {pid:#010x} has no list entry"),
                )
            })?;
        let name = std::str::from_utf8(name)
            .map_err(|_| ParseError::new("PRO lookup", 0, "non-UTF-8 prototype filename"))?;
        let path = format!("proto/{folder}/{name}");
        let prototype = Prototype::parse_for(&self.read(&path)?, self.game)?;
        // The list selects the file; its own PID selects its serialized layout.
        // SPEC-003 §3.1 safe rule: a stored PID that differs from the
        // requested PID is rejected.
        if prototype.pid != pid {
            return Err(ParseError::new(
                "PRO lookup",
                0,
                format!("{path} stores PID {:#010x}, not {pid:#010x}", prototype.pid),
            ));
        }
        let info = prototype.info();
        self.prototypes.borrow_mut().insert(pid, info);
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ByteSource, MemoryDir};

    /// A synthetic item prototype whose stored PID is `pid`.
    fn item(pid: u32) -> ByteSource {
        let mut bytes = vault_fixtures::world::prototype(0, 1);
        bytes[..4].copy_from_slice(&pid.to_be_bytes());
        ByteSource::new(bytes).unwrap()
    }

    fn registration(category: u8, file: &str, index: u32) -> ProtoRegistration {
        ProtoRegistration {
            category,
            file: file.into(),
            index,
        }
    }

    #[test]
    fn extended_registered_prototype_rows_resolve_past_the_list() {
        let dir = MemoryDir::new([
            (
                "proto/items/items.lst",
                ByteSource::new(b"00000001.pro\n00000002.pro\n".to_vec()).unwrap(),
            ),
            ("proto/items/00000001.pro", item(1)),
            ("proto/items/00000002.pro", item(2)),
            ("proto/items/00005000.pro", item(5000)),
            ("proto/items/shadow.pro", item(2)),
        ])
        .unwrap();
        // Without registrations, as today: entry 5000 has no list entry.
        let plain = LocalPrototypeCatalog::new(&dir);
        assert!(plain.lookup(1).is_ok());
        let error = plain.lookup(5000).unwrap_err();
        assert!(error.message.contains("has no list entry"), "{error:?}");
        assert_eq!(plain.registered_file(5000), None);
        let catalog = LocalPrototypeCatalog::new(&dir).with_registered([
            registration(0, "00005000.pro", 5000),
            // An entry the list file holds keeps its list row.
            registration(0, "shadow.pro", 2),
            // Another category's index 5000 is another PID.
            registration(2, "missing.pro", 5000),
        ]);
        assert_eq!(
            catalog.lookup(5000).unwrap(),
            Prototype::parse(item(5000).as_slice()).unwrap().info()
        );
        assert_eq!(catalog.registered_file(5000), Some("00005000.pro"));
        assert_eq!(catalog.registered_file(0x0200_1388), Some("missing.pro"));
        assert!(catalog.lookup(2).is_ok(), "the list's row 2");
        assert!(catalog.lookup(4999).is_err(), "a gap entry has no file");
        assert!(catalog.lookup(5001).is_err());
    }
}
