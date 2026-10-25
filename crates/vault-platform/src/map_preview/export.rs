//! The F9 overlay's "Export Current Setup as Mod": a ready-to-share mod
//! folder with a `vault-mod.toml` of the running `enhanced` presentation,
//! the setup's table overrides and enhanced scripts
//! ([`vault_core::mod_export::setup_files`]) and a `README.txt`. Engine
//! extension, not part of any specification.
use super::MapPreview;
use std::path::PathBuf;
use vault_formats::{ModManifest, mod_manifest};

/// The exported folder's name under the export root (`-2`, `-3`, ... when
/// taken).
pub(crate) const EXPORT_SLUG: &str = "enhanced-setup";

impl MapPreview {
    /// The running setup as a manifest: the `enhanced` preset with its
    /// viewport, smoothing and roof peek as they are now (an Alt+R change
    /// included), and the zoom the camera is at (or heading for) as the
    /// zoom a map starts at, to two places. `None` under Vanilla.
    pub(crate) fn setup_manifest(&self) -> Option<ModManifest> {
        let settings = self.enhanced()?;
        let zoom = self
            .unlocked()
            .map_or(f64::from(settings.default_zoom), |enhanced| {
                enhanced.camera.zoom_goal()
            });
        Some(crate::mods::setup_manifest(
            "Exported enhanced setup",
            "The enhanced viewport, camera and roof-peek settings (with any table overrides \
             and enhanced scripts in use), exported from the F9 overlay.",
            &settings,
            zoom,
        ))
    }
    /// Export the running setup to a fresh folder under the export root
    /// ([`crate::mods::export_root`]); returns the folder. Refused without
    /// an export root (a replay, a preview tool) or under Vanilla.
    pub(crate) fn export_setup(&mut self) -> Result<PathBuf, String> {
        let root = self
            .mod_export
            .clone()
            .ok_or("exports are off in this session")?;
        let manifest = self
            .setup_manifest()
            .ok_or("the enhanced preset is off: nothing to export")?;
        let files = vault_core::mod_export::setup_files(self.host.assets.vfs.as_ref())
            .map_err(|error| error.to_string())?;
        let output = crate::mods::free_folder(&root, EXPORT_SLUG);
        mod_manifest::write_mod_folder(&output, &manifest, &files)
            .map_err(|error| format!("{}: {error}", output.display()))?;
        tracing::info!(folder = %output.display(), files = files.len(), "setup exported as a mod");
        Ok(output)
    }
}
