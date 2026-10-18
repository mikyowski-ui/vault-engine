use crate::{WorldError, dialogue::HeadDefinition};
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};
use vault_formats::{
    ArtCatalog, Fid, Font, FontKind, FontSet, FrmArchive, GamFile, GamSection, GcdFile,
    LocalPrototypeCatalog, LstKind, LstTable, MAX_ASSET_BYTES, MapFile, MsgCatalog, Prototype, Vfs,
    VfsPath, pro::Category,
};
use vault_vm::IntProgram;

/// Resource policy is explicit. Numeric perk IDs/XP curves and other tables not
/// specified by SPEC-005/006 belong to WorldRules, not inferred bytecode constants.
pub trait WorldAssets {
    fn prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError>;
    fn program(&self, index: i32) -> Result<(String, IntProgram), WorldError>;
    /// SPEC-004 launch operand: a program filename, resolved below `scripts/`.
    fn program_by_name(&self, _name: &[u8]) -> Result<(String, IntProgram), WorldError> {
        Err(WorldError::Invalid(
            "program launch resources unavailable".into(),
        ))
    }
    fn local_count(&self, _index: i32) -> usize {
        0
    }
    fn message(&self, list: i32, id: i32) -> Option<vault_formats::msg::Message>;
    fn prototype_text(&self, _prototype: &Prototype, _description: bool) -> Option<Vec<u8>> {
        None
    }
    fn head(&self, _index: i32) -> Result<HeadDefinition, WorldError> {
        Err(WorldError::Invalid("head resource unavailable".into()))
    }
    fn frame(&self, _path: &str) -> Result<Arc<FrmArchive>, WorldError> {
        Err(WorldError::Invalid("frame resource unavailable".into()))
    }
    fn background_path(&self, _index: i32) -> Result<String, WorldError> {
        Err(WorldError::Invalid(
            "background resource unavailable".into(),
        ))
    }
    fn interface_path(&self, _index: i32) -> Result<String, WorldError> {
        Err(WorldError::Invalid("interface resource unavailable".into()))
    }
    fn font(&self) -> Result<Arc<Font>, WorldError> {
        Err(WorldError::Invalid("font resource unavailable".into()))
    }
    /// A palette/color-table resource requested by `loadpalettetable`.
    fn palette(&self, _name: &[u8]) -> Result<vault_formats::Palette, WorldError> {
        Err(WorldError::Invalid("palette resource unavailable".into()))
    }
    /// A loaded font by SPEC-002 selection number (FON 0-9, AAF 100-110).
    fn font_number(&self, _number: i32) -> Option<Arc<Font>> {
        None
    }
    fn gam(&self, _path: &str, _section: GamSection) -> Result<Option<GamFile>, WorldError> {
        Ok(None)
    }
    fn map(&self, _path: &str) -> Result<MapFile, WorldError> {
        Err(WorldError::Invalid("map resource unavailable".into()))
    }
    fn map_index(&self, _name: &[u8]) -> Option<i32> {
        None
    }
    fn available(&self, _path: &[u8]) -> bool {
        true
    }
    /// The `fallout.cfg` `[system]` language directory below `text/`.
    fn language(&self) -> &str {
        "english"
    }
    fn files(&self, _pattern: &[u8]) -> Vec<Vec<u8>> {
        Vec::new()
    }
    fn dialogue_label(&self, done: bool) -> Vec<u8> {
        if done {
            b"Done".to_vec()
        } else {
            b"Continue".to_vec()
        }
    }
    fn release_dialogue_art(&self) {}
    /// The unaliased art-list token at an art index (SPEC-010 effect stems).
    fn art_token(&self, _category: u8, _index: u16) -> Option<Vec<u8>> {
        None
    }
    /// Load the art selected by a complete SPEC-007 identifier.
    fn object_art(&self, _fid: u32) -> Result<Arc<FrmArchive>, WorldError> {
        Err(WorldError::Invalid("object art unavailable".into()))
    }
    /// SPEC-007 constructor. Without art resources every existence probe
    /// fails, which encodes selector 0.
    fn construct_fid(
        &self,
        category: u8,
        base: u16,
        animation: u8,
        weapon: u8,
        _selector: u8,
    ) -> u32 {
        Fid::new(category, base, animation, weapon, 0).0
    }
    /// The SPEC-007 existence test for the file an identifier selects.
    fn art_exists(&self, fid: u32) -> bool {
        self.object_art(fid).is_ok()
    }
    /// A raw resource by logical path (for example `data/ai.txt`).
    fn resource(&self, path: &str) -> Result<Vec<u8>, WorldError> {
        Err(WorldError::Invalid(format!("{path}: resource unavailable")))
    }
    /// A localized game message catalog (`text/<language>/game/<file>`).
    fn game_messages(&self, file: &str) -> Result<Arc<MsgCatalog>, WorldError> {
        Err(WorldError::Invalid(format!("{file}: messages unavailable")))
    }
}
pub struct VfsWorldAssets {
    pub vfs: Arc<dyn Vfs>,
    pub language: String,
    prototypes: RefCell<BTreeMap<u32, Arc<Prototype>>>,
    messages: RefCell<BTreeMap<String, MsgCatalog>>,
    game_catalogs: RefCell<BTreeMap<String, Arc<MsgCatalog>>>,
    programs: RefCell<BTreeMap<i32, (String, IntProgram)>>,
    frames: RefCell<BTreeMap<String, Arc<FrmArchive>>>,
    frame_bytes: RefCell<usize>,
    art: RefCell<Option<Arc<ArtCatalog>>>,
    fonts: RefCell<Option<Arc<FontSet>>>,
}
impl VfsWorldAssets {
    pub fn new(vfs: Arc<dyn Vfs>, language: &str) -> Result<Self, WorldError> {
        if language.is_empty()
            || !language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(WorldError::Invalid(
                "language must be a resource directory name".into(),
            ));
        }
        Ok(Self {
            vfs,
            language: language.to_ascii_lowercase(),
            prototypes: RefCell::new(BTreeMap::new()),
            messages: RefCell::new(BTreeMap::new()),
            game_catalogs: RefCell::new(BTreeMap::new()),
            programs: RefCell::new(BTreeMap::new()),
            frames: RefCell::new(BTreeMap::new()),
            frame_bytes: RefCell::new(0),
            art: RefCell::new(None),
            fonts: RefCell::new(None),
        })
    }
    /// Category art lists, parsed once on first use.
    pub fn art_catalog(&self) -> Result<Arc<ArtCatalog>, WorldError> {
        if let Some(catalog) = self.art.borrow().as_ref() {
            return Ok(catalog.clone());
        }
        let catalog = Arc::new(ArtCatalog::load(&*self.vfs)?);
        *self.art.borrow_mut() = Some(catalog.clone());
        Ok(catalog)
    }
    /// SPEC-002 startup font search, performed once on first use.
    pub fn font_set(&self) -> Arc<FontSet> {
        self.fonts
            .borrow_mut()
            .get_or_insert_with(|| Arc::new(FontSet::load(&*self.vfs)))
            .clone()
    }
    fn exists(&self, path: &str) -> bool {
        VfsPath::new(path).is_ok_and(|path| self.vfs.files().any(|file| *file == path))
    }
    pub fn read(&self, path: &str) -> Result<Vec<u8>, WorldError> {
        self.vfs
            .read_bounded(path, MAX_ASSET_BYTES)
            .map_err(|error| WorldError::Invalid(format!("{path}: {error}")))
    }
    fn list(&self, path: &str, kind: LstKind) -> Result<LstTable, WorldError> {
        Ok(LstTable::parse(&self.read(path)?, kind)?)
    }
    fn art_name(&self, folder: &str, index: i32) -> Result<String, WorldError> {
        if !(0..4096).contains(&index) {
            return Err(WorldError::Invalid(
                "art index outside supported range".into(),
            ));
        }
        let list = self.list(&format!("art/{folder}/{folder}.lst"), LstKind::Art)?;
        let name = list
            .art_id(index as u32)
            .and_then(|entry| entry.name.as_deref())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| WorldError::Invalid(format!("missing {folder} art index {index}")))?;
        Ok(format!(
            "art/{folder}/{}",
            std::str::from_utf8(name)
                .map_err(|_| WorldError::Invalid("invalid art filename".into()))?
        ))
    }
    fn catalog(&self, path: &str, id: i32) -> Option<vault_formats::msg::Message> {
        if !self.messages.borrow().contains_key(path) {
            let catalog = MsgCatalog::parse(&self.read(path).ok()?).ok()?;
            self.messages.borrow_mut().insert(path.into(), catalog);
        }
        self.messages.borrow().get(path)?.get(id).cloned()
    }
    pub fn gcd(&self, path: &str) -> Result<GcdFile, WorldError> {
        Ok(GcdFile::parse(&self.read(path)?)?)
    }
}
impl WorldAssets for VfsWorldAssets {
    fn prototype(&self, pid: u32) -> Result<Arc<Prototype>, WorldError> {
        if let Some(prototype) = self.prototypes.borrow().get(&pid) {
            return Ok(prototype.clone());
        }
        let category = Category::from_pid(pid)?;
        let folder = category.directory();
        let list = self.list(&format!("proto/{folder}/{folder}.lst"), LstKind::Prototype)?;
        let name = list
            .prototype_id(pid)
            .and_then(|entry| entry.name.as_deref())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| WorldError::Invalid(format!("prototype {pid:#x} has no list entry")))?;
        let path = format!(
            "proto/{folder}/{}",
            std::str::from_utf8(name)
                .map_err(|_| WorldError::Invalid("invalid prototype filename".into()))?
        );
        let prototype = Arc::new(Prototype::parse(&self.read(&path)?)?);
        self.prototypes.borrow_mut().insert(pid, prototype.clone());
        Ok(prototype)
    }
    fn program(&self, index: i32) -> Result<(String, IntProgram), WorldError> {
        if let Some(program) = self.programs.borrow().get(&index) {
            return Ok(program.clone());
        }
        if index < 0 {
            return Err(WorldError::Invalid("negative script-file index".into()));
        }
        let list = self.list("scripts/scripts.lst", LstKind::Script)?;
        let name = list
            .script_id(index as u32)
            .and_then(|entry| entry.name.as_deref())
            .ok_or_else(|| WorldError::Invalid(format!("script index {index} missing")))?;
        let path = format!(
            "scripts/{}",
            std::str::from_utf8(name)
                .map_err(|_| WorldError::Invalid("invalid script name".into()))?
        );
        let program = IntProgram::parse(&self.read(&path)?)?;
        self.programs
            .borrow_mut()
            .insert(index, (path.clone(), program.clone()));
        Ok((path, program))
    }
    fn program_by_name(&self, name: &[u8]) -> Result<(String, IntProgram), WorldError> {
        let name = std::str::from_utf8(name)
            .ok()
            .filter(|name| name.is_ascii())
            .ok_or_else(|| WorldError::Invalid("program filename is not ASCII".into()))?;
        let path = VfsPath::new(name)
            .map_err(|error| WorldError::Invalid(format!("program filename: {error}")))?;
        let path = if path.as_str().starts_with("scripts/") {
            path.to_string()
        } else {
            format!("scripts/{path}")
        };
        let program = IntProgram::parse(&self.read(&path)?)?;
        Ok((path, program))
    }
    fn local_count(&self, index: i32) -> usize {
        if index < 0 {
            return 0;
        }
        self.list("scripts/scripts.lst", LstKind::Script)
            .ok()
            .and_then(|list| {
                list.script_id(index as u32)
                    .and_then(|entry| entry.local_vars)
            })
            .filter(|count| *count >= 0 && *count <= 1_000_000)
            .unwrap_or(0) as usize
    }
    fn message(&self, list: i32, id: i32) -> Option<vault_formats::msg::Message> {
        if list <= 0 {
            return None;
        }
        let scripts = self.list("scripts/scripts.lst", LstKind::Script).ok()?;
        let name = scripts.script_id((list - 1) as u32)?.name.as_deref()?;
        let stem = std::str::from_utf8(name).ok()?.strip_suffix(".int")?;
        self.catalog(&format!("text/{}/dialog/{stem}.msg", self.language), id)
    }
    fn prototype_text(&self, prototype: &Prototype, description: bool) -> Option<Vec<u8>> {
        if prototype.message_base == -1 {
            return None;
        }
        let stem = match prototype.category {
            Category::Item => "item",
            Category::Critter => "crit",
            Category::Scenery => "scen",
            Category::Wall => "wall",
            Category::Tile => "tile",
            Category::Misc => "misc",
        };
        self.catalog(
            &format!("text/{}/game/pro_{stem}.msg", self.language),
            prototype.message_base.checked_add(i32::from(description))?,
        )
        .map(|message| message.text)
    }
    fn head(&self, index: i32) -> Result<HeadDefinition, WorldError> {
        if !(0..4096).contains(&index) {
            return Err(WorldError::Invalid("invalid head index".into()));
        }
        let list = self.list("art/heads/heads.lst", LstKind::Art)?;
        let entry = list
            .art_id(index as u32)
            .ok_or_else(|| WorldError::Invalid("missing head".into()))?;
        let stem = std::str::from_utf8(entry.name.as_deref().unwrap_or(b""))
            .map_err(|_| WorldError::Invalid("invalid head stem".into()))?;
        let counts = entry
            .head_counts
            .ok_or_else(|| WorldError::Invalid("head has no fidget counts".into()))?;
        Ok(HeadDefinition {
            index: index as u16,
            stem: stem.into(),
            counts,
        })
    }
    fn frame(&self, path: &str) -> Result<Arc<FrmArchive>, WorldError> {
        if let Some(frame) = self.frames.borrow().get(path) {
            return Ok(frame.clone());
        }
        let bytes = self.read(path)?;
        let total = self
            .frame_bytes
            .borrow()
            .checked_add(bytes.len())
            .ok_or(WorldError::Arithmetic)?;
        if total > 256 * 1024 * 1024 {
            return Err(WorldError::Invalid(
                "dialogue art cache limit exceeded".into(),
            ));
        }
        let frame = Arc::new(FrmArchive::parse(&bytes)?);
        self.frames.borrow_mut().insert(path.into(), frame.clone());
        *self.frame_bytes.borrow_mut() = total;
        Ok(frame)
    }
    fn object_art(&self, fid: u32) -> Result<Arc<FrmArchive>, WorldError> {
        let path = self
            .art_catalog()?
            .path(Fid(fid))
            .map_err(|error| WorldError::Invalid(format!("art {fid:#010x}: {error}")))?;
        self.frame(&path.to_ascii_lowercase())
    }
    fn construct_fid(
        &self,
        category: u8,
        base: u16,
        animation: u8,
        weapon: u8,
        selector: u8,
    ) -> u32 {
        match self.art_catalog() {
            Ok(catalog) => {
                catalog
                    .construct(category, base, animation, weapon, selector, |path| {
                        self.exists(path)
                    })
                    .0
            }
            Err(_) => Fid::new(category, base, animation, weapon, 0).0,
        }
    }
    fn art_token(&self, category: u8, index: u16) -> Option<Vec<u8>> {
        Some(
            self.art_catalog()
                .ok()?
                .base_token(category, index)?
                .to_vec(),
        )
    }
    fn art_exists(&self, fid: u32) -> bool {
        self.art_catalog()
            .ok()
            .and_then(|catalog| catalog.path(Fid(fid)).ok())
            .is_some_and(|path| self.exists(&path))
    }
    fn resource(&self, path: &str) -> Result<Vec<u8>, WorldError> {
        self.read(path)
    }
    fn game_messages(&self, file: &str) -> Result<Arc<MsgCatalog>, WorldError> {
        let path = format!("text/{}/game/{file}", self.language);
        if let Some(catalog) = self.game_catalogs.borrow().get(&path) {
            return Ok(catalog.clone());
        }
        let catalog = Arc::new(MsgCatalog::parse(&self.read(&path)?)?);
        self.game_catalogs
            .borrow_mut()
            .insert(path, catalog.clone());
        Ok(catalog)
    }
    fn background_path(&self, index: i32) -> Result<String, WorldError> {
        self.art_name("backgrnd", index)
    }
    fn interface_path(&self, index: i32) -> Result<String, WorldError> {
        self.art_name("intrface", index)
    }
    /// Presentation text uses the AAF subsystem's initial font when it
    /// initialized, otherwise the FON subsystem's.
    fn font(&self) -> Result<Arc<Font>, WorldError> {
        let fonts = self.font_set();
        [FontKind::Antialiased, FontKind::Bitmap]
            .into_iter()
            .find_map(|kind| fonts.default_font(kind).map(|(_, font)| font.clone()))
            .ok_or_else(|| WorldError::Invalid("no dialogue font".into()))
    }
    fn font_number(&self, number: i32) -> Option<Arc<Font>> {
        self.font_set().get(number).cloned()
    }
    fn palette(&self, name: &[u8]) -> Result<vault_formats::Palette, WorldError> {
        let name = std::str::from_utf8(name)
            .map_err(|_| WorldError::Invalid("palette filename is not ASCII".into()))?;
        Ok(vault_formats::Palette::parse(&self.read(name)?)?)
    }
    fn gam(&self, path: &str, section: GamSection) -> Result<Option<GamFile>, WorldError> {
        match self.vfs.read_bounded(path, MAX_ASSET_BYTES) {
            Ok(bytes) => Ok(Some(GamFile::parse(&bytes, section)?)),
            Err(vault_formats::VfsError::NotFound(_)) => Ok(None),
            Err(error) => Err(WorldError::Invalid(error.to_string())),
        }
    }
    fn map(&self, path: &str) -> Result<MapFile, WorldError> {
        let saved = path.strip_suffix(".map").map(|stem| format!("{stem}.sav"));
        if let Some(saved) = saved
            && let Ok(bytes) = self.read(&saved)
            && let Ok(map) = MapFile::parse(&bytes, &LocalPrototypeCatalog::new(&*self.vfs))
        {
            return Ok(map);
        }
        Ok(MapFile::parse(
            &self.read(path)?,
            &LocalPrototypeCatalog::new(&*self.vfs),
        )?)
    }
    fn map_index(&self, name: &[u8]) -> Option<i32> {
        let text = std::str::from_utf8(name).ok()?.to_ascii_lowercase();
        for path in self
            .vfs
            .files()
            .filter(|path| path.extension() == Some("map"))
        {
            let file = path.as_str().rsplit('/').next()?;
            if file == text || file.strip_suffix(".map") == Some(text.as_str()) {
                let bytes = self.read(path.as_str()).ok()?;
                return Some(i32::from_be_bytes(bytes.get(0x34..0x38)?.try_into().ok()?));
            }
        }
        None
    }
    /// Either slash style; names compare case-insensitively.
    fn available(&self, path: &[u8]) -> bool {
        std::str::from_utf8(path).is_ok_and(|path| self.exists(path))
    }
    fn language(&self) -> &str {
        &self.language
    }
    fn files(&self, pattern: &[u8]) -> Vec<Vec<u8>> {
        let Ok(pattern) = std::str::from_utf8(pattern) else {
            return Vec::new();
        };
        let (prefix, suffix) = pattern.split_once('*').unwrap_or((pattern, ""));
        self.vfs
            .files()
            .filter(|path| {
                path.as_str().starts_with(&prefix.to_ascii_lowercase())
                    && path.as_str().ends_with(&suffix.to_ascii_lowercase())
            })
            .take(10000)
            .map(|path| path.as_str().as_bytes().to_vec())
            .collect()
    }
    fn release_dialogue_art(&self) {
        self.frames.borrow_mut().retain(|path, _| {
            !["art/heads/", "art/backgrnd/", "art/intrface/"]
                .iter()
                .any(|prefix| path.starts_with(prefix))
        });
        *self.frame_bytes.borrow_mut() = self
            .frames
            .borrow()
            .values()
            .map(|frame| frame.stored_frame_area_len() as usize + 62)
            .sum();
    }
}
