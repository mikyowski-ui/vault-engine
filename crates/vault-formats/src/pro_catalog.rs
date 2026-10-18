use crate::{
    LstKind, LstTable, MAX_ASSET_BYTES, ParseError, Prototype, Vfs,
    pro::{Category, PrototypeInfo, PrototypeLookup},
};
use std::{cell::RefCell, collections::BTreeMap};

/// On-demand, cached subtype resolution through one-based prototype lists.
pub struct LocalPrototypeCatalog<'a> {
    vfs: &'a dyn Vfs,
    lists: RefCell<BTreeMap<Category, LstTable>>,
    prototypes: RefCell<BTreeMap<u32, PrototypeInfo>>,
}
impl<'a> LocalPrototypeCatalog<'a> {
    pub fn new(vfs: &'a dyn Vfs) -> Self {
        Self {
            vfs,
            lists: RefCell::new(BTreeMap::new()),
            prototypes: RefCell::new(BTreeMap::new()),
        }
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
        let name = lists[&category]
            .prototype_id(pid)
            .and_then(|entry| entry.name.as_deref())
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
        let prototype = Prototype::parse(&self.read(&path)?)?;
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
