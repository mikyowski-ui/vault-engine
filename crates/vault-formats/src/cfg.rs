//! `fallout.cfg`: `[section]` headers and `key=value` lines (SPEC-014
//! §2.3 lists the consumed keys and defaults). Whitespace around names and
//! values is trimmed; blank lines, comments (`;` or `#`) and lines without
//! `=` outside a section are ignored. Unknown keys are kept and written back.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigFile {
    sections: Vec<(String, Vec<(String, String)>)>,
}

impl ConfigFile {
    pub fn parse(text: &str) -> Self {
        let mut config = Self::default();
        let mut section: Option<String> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = Some(name.trim().to_owned());
                continue;
            }
            if let (Some(section), Some((key, value))) = (&section, line.split_once('=')) {
                config.set(section, key.trim(), value.trim());
            }
        }
        config
    }
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .iter()
            .find(|(name, _)| name == section)?
            .1
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
    pub fn get_int(&self, section: &str, key: &str) -> Option<i64> {
        self.get(section, key)?.parse().ok()
    }
    /// A real value. R-05: `nan`, `inf` and `infinity` parse as `f64` but
    /// are no configuration value; they read as missing, so the caller's
    /// default applies (SPEC-014 §2.3 clamps only finite values).
    pub fn get_real(&self, section: &str, key: &str) -> Option<f64> {
        self.get(section, key)?
            .parse()
            .ok()
            .filter(|value: &f64| value.is_finite())
    }
    /// Replace a value, adding the section or key in first-use order.
    pub fn set(&mut self, section: &str, key: &str, value: impl Into<String>) {
        let value = value.into();
        let index = match self.sections.iter().position(|(name, _)| name == section) {
            Some(index) => index,
            None => {
                self.sections.push((section.to_owned(), Vec::new()));
                self.sections.len() - 1
            }
        };
        let entries = &mut self.sections[index].1;
        match entries.iter_mut().find(|(name, _)| name == key) {
            Some(entry) => entry.1 = value,
            None => entries.push((key.to_owned(), value)),
        }
    }
    /// Sections and keys in stored order, a blank line between sections.
    pub fn write(&self) -> String {
        let mut out = String::new();
        for (index, (name, entries)) in self.sections.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            out.push_str(&format!("[{name}]\n"));
            for (key, value) in entries {
                out.push_str(&format!("{key}={value}\n"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::ConfigFile;

    #[test]
    fn reads_sections_keys_and_writes_them_back() {
        let config = ConfigFile::parse(
            "; comment\r\n[system]\r\nsplash = 3\r\nlanguage=english\r\n\r\n[preferences]\r\nbrightness=1.011250\r\nstray\r\n",
        );
        assert_eq!(config.get_int("system", "splash"), Some(3));
        assert_eq!(config.get("system", "language"), Some("english"));
        assert_eq!(config.get_real("preferences", "brightness"), Some(1.01125));
        assert_eq!(config.get("preferences", "missing"), None);
        let mut written = config.clone();
        written.set("system", "splash", "4");
        written.set("sound", "master_volume", "22281");
        let text = written.write();
        assert_eq!(
            text,
            "[system]\nsplash=4\nlanguage=english\n\n[preferences]\nbrightness=1.011250\n\n[sound]\nmaster_volume=22281\n"
        );
        assert_eq!(ConfigFile::parse(&text), written);
    }

    #[test]
    fn non_finite_reals_read_as_missing() {
        let config = ConfigFile::parse(
            "[preferences]\nbrightness=nan\ntext_base_delay=inf\nmouse_sensitivity=-Infinity\n",
        );
        for key in ["brightness", "text_base_delay", "mouse_sensitivity"] {
            assert_eq!(config.get_real("preferences", key), None, "{key}");
        }
    }
}
