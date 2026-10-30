//! SPEC-024b §4.5: the INI files scripts read. A setting names one value as
//! `file|section|key`. Files are inputs: a replay needs the same ones.
use std::collections::BTreeMap;

/// Where `set_ini_setting` keeps its files, and where a file the VFS layers
/// do not hold is looked for: the profile's writable folder.
pub trait IniFiles: Send {
    fn read(&self, file: &str) -> Option<Vec<u8>>;
    fn write(&mut self, file: &str, text: &[u8]) -> Result<(), String>;
}

/// `file|section|key`, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub file: String,
    pub section: String,
    pub key: String,
}
/// A file name the rules accept: at most 63 bytes, with no `..`, no absolute
/// path and no drive prefix.
pub fn valid_file(file: &[u8]) -> Option<String> {
    let name = std::str::from_utf8(file).ok()?;
    let bytes = name.as_bytes();
    let absolute = bytes.first().is_some_and(|byte| b"/\\".contains(byte));
    let drive = bytes.get(1) == Some(&b':');
    (!name.is_empty() && name.len() <= 63 && !name.contains("..") && !absolute && !drive)
        .then(|| name.to_owned())
}
impl Setting {
    /// Exactly three fields; `None` is a malformed setting.
    pub fn parse(text: &[u8]) -> Option<Self> {
        let fields: Vec<&[u8]> = text.split(|byte| *byte == b'|').collect();
        let [file, section, key] = fields[..] else {
            return None;
        };
        if section.len() > 32 {
            return None;
        }
        Some(Self {
            file: valid_file(file)?,
            section: String::from_utf8(section.to_vec()).ok()?,
            key: String::from_utf8(key.to_vec()).ok()?,
        })
    }
}

/// A section's pairs, in file order. Names compare without case, and keep
/// their first spelling.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IniSection {
    pub name: String,
    pub pairs: Vec<(String, String)>,
}
impl IniSection {
    fn slot(&mut self, key: &str) -> Option<&mut (String, String)> {
        self.pairs
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_str())
    }
}
/// One file, in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IniFile {
    pub sections: Vec<IniSection>,
}
impl IniFile {
    pub fn parse(text: &[u8]) -> Self {
        let text = String::from_utf8_lossy(text);
        let mut file = Self::default();
        let mut current: Option<usize> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with([';', '#']) {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.split(']').next()) {
                current = Some(file.section_index(name.trim()));
            } else if let (Some(at), Some((key, value))) = (current, line.split_once('=')) {
                let value = value.split(';').next().unwrap_or("").trim();
                file.sections[at].set(key.trim(), value);
            }
        }
        file
    }
    fn section_index(&mut self, name: &str) -> usize {
        self.sections
            .iter()
            .position(|section| section.name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| {
                self.sections.push(IniSection {
                    name: name.to_owned(),
                    pairs: Vec::new(),
                });
                self.sections.len() - 1
            })
    }
    pub fn section(&self, name: &str) -> Option<&IniSection> {
        self.sections
            .iter()
            .find(|section| section.name.eq_ignore_ascii_case(name))
    }
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.section(section)?.get(key)
    }
    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let at = self.section_index(section);
        self.sections[at].set(key, value);
    }
    /// The file's text, with its order kept.
    pub fn write(&self) -> Vec<u8> {
        let mut text = String::new();
        for section in &self.sections {
            text.push_str(&format!("[{}]\n", section.name));
            for (key, value) in &section.pairs {
                text.push_str(&format!("{key}={value}\n"));
            }
        }
        text.into_bytes()
    }
}
impl IniSection {
    fn set(&mut self, key: &str, value: &str) {
        match self.slot(key) {
            Some(pair) => pair.1 = value.to_owned(),
            None => self.pairs.push((key.to_owned(), value.to_owned())),
        }
    }
}
/// The files read so far, each read once for the session; `None` is a file
/// that is absent.
pub type IniCache = BTreeMap<String, Option<IniFile>>;
