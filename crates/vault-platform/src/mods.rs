//! Mod layers' `vault-mod.toml` manifests on the desktop: which enabled
//! folder layers carry one, applying their `[enhanced]` settings to a
//! launch, and where the F9 overlay exports the current setup as a mod.
//! Engine extension, not part of any specification.
use crate::config::{self, Config, ModList, Preset};
use crate::enhanced::{EnhancedSettings, ViewportMode};
use crate::storage::{self, FsStorage, Namespace, StorageBackend};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use vault_core::mod_export::Setup;
use vault_formats::{
    EnhancedOverrides, LayerEntry, LoadOrderPolicy, ModManifest, Vfs,
    mod_manifest::{self, MANIFEST},
};

/// The folder a load-order entry mounts, resolved against `directory` as
/// the mount would resolve it: `None` for an archive-only entry, or a path
/// the `policy` refuses (the mount reports that).
pub fn layer_dir(directory: &Path, entry: &LayerEntry, policy: LoadOrderPolicy) -> Option<PathBuf> {
    let path = entry.path.as_deref()?;
    if path.trim().is_empty() || path.to_ascii_lowercase().ends_with(".dat") {
        return None;
    }
    let rooted = Path::new(path).is_absolute()
        || path.starts_with(['/', '\\'])
        || path.as_bytes().get(1) == Some(&b':');
    let traversal = path.split(['/', '\\']).any(|part| part == "..");
    if (rooted || traversal) && !policy.allow_outside {
        return None;
    }
    Some(directory.join(path))
}

/// The enabled folder layers of `mod_list`, lowest first. A load order
/// that cannot be read lists none (mounting it reports why).
pub fn layer_dirs(mod_list: &ModList) -> Vec<PathBuf> {
    match mod_list {
        ModList::Paths(paths) => paths
            .iter()
            .filter(|path| {
                !path
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".dat")
            })
            .cloned()
            .collect(),
        ModList::File(file) => {
            let text = storage::read_config_file(file).ok().flatten();
            let Some(entries) = text.and_then(|text| vault_formats::parse_load_order(&text).ok())
            else {
                return Vec::new();
            };
            let directory = match file.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                _ => Path::new("."),
            };
            entries
                .iter()
                .filter(|entry| entry.enabled)
                .filter_map(|entry| layer_dir(directory, entry, LoadOrderPolicy::default()))
                .collect()
        }
    }
}

/// The manifest at the root of layer folder `dir`, if it has one. A
/// malformed one, or a link in its place, is an error naming the folder.
pub fn read_manifest(dir: &Path) -> Result<Option<ModManifest>, String> {
    let path = dir.join(MANIFEST);
    // A folder in its place is no entry, but it is no manifest either.
    if path.is_dir() {
        return Err(format!("{}: not a regular file", path.display()));
    }
    let bytes = FsStorage::mods_at(dir)
        .read(Namespace::Mods, MANIFEST)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    let text = String::from_utf8(bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    ModManifest::parse(&text)
        .map(Some)
        .map_err(|error| format!("{}: {error}", dir.display()))
}

/// `manifest`'s `[enhanced]` settings over `config` (each key it sets
/// replaces the settings file's).
pub fn apply(config: &mut Config, manifest: &ModManifest) {
    let Some(enhanced) = &manifest.enhanced else {
        return;
    };
    if let Some(preset) = enhanced.preset.as_deref().and_then(Preset::parse) {
        config.preset = Some(preset);
    }
    if let Some(mode) = enhanced
        .viewport_mode
        .as_deref()
        .and_then(config::parse_viewport)
    {
        config.viewport_mode = Some(mode);
    }
    if let Some(zoom) = enhanced.default_zoom {
        config.default_zoom = Some(zoom);
    }
    if let Some(smooth) = enhanced.smooth_camera {
        config.smooth_camera = Some(smooth);
    }
    if let Some(mode) = enhanced
        .roof_peek
        .as_deref()
        .and_then(config::parse_roof_peek)
    {
        config.roof_peek = Some(mode);
    }
}

/// `config` with the `[enhanced]` settings of every enabled layer's
/// manifest applied, lowest layer first (so the highest wins a key, as its
/// files do), and the names of the mods that carried settings. The command
/// line still wins over the result ([`crate`]'s launch resolution). A
/// malformed manifest stops the launch.
pub fn with_mods(config: &Config) -> Result<(Config, Vec<String>), String> {
    let mut merged = config.clone();
    let mut applied = Vec::new();
    let Some(mod_list) = &config.mod_list else {
        return Ok((merged, applied));
    };
    for dir in layer_dirs(mod_list) {
        if let Some(manifest) = read_manifest(&dir)? {
            if manifest.enhanced.is_some() {
                applied.push(manifest.name.clone());
            }
            apply(&mut merged, &manifest);
        }
    }
    Ok((merged, applied))
}

/// Where the F9 overlay exports mods: `mods/` beside the load order the
/// settings name, else in the working directory.
pub fn export_root(mod_list: Option<&ModList>) -> PathBuf {
    match mod_list {
        Some(ModList::File(file)) => match file.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.join("mods"),
            _ => PathBuf::from("mods"),
        },
        _ => PathBuf::from("mods"),
    }
}

/// The manifest of an exported setup named `name`: the `enhanced` preset
/// with `settings`' viewport, smoothing and roof peek, and `zoom` (to two
/// places, in range) as the zoom a map starts at.
pub fn setup_manifest(
    name: &str,
    description: &str,
    settings: &EnhancedSettings,
    zoom: f64,
) -> ModManifest {
    let range = mod_manifest::ZOOM_RANGE;
    let zoom = (((zoom * 100.0).round() / 100.0) as f32).clamp(*range.start(), *range.end());
    ModManifest {
        name: name.into(),
        version: Some("1.0.0".into()),
        author: None,
        description: Some(description.into()),
        enhanced: Some(EnhancedOverrides {
            preset: Some("enhanced".into()),
            viewport_mode: Some(config::viewport_name(settings.viewport).into()),
            default_zoom: Some(zoom),
            smooth_camera: Some(settings.smooth_camera),
            roof_peek: Some(config::roof_peek_name(settings.roof_peek).into()),
        }),
        ..ModManifest::default()
    }
}

/// `--export-mod` (and `cargo xtask export-mod`): what to export and the
/// command line's overrides.
#[derive(Debug, Clone, Default)]
pub struct ExportRequest {
    /// The mod's folder name and manifest name.
    pub name: String,
    /// The folder to write; default `mods/<name>` ([`export_root`]).
    pub output: Option<PathBuf>,
    /// Extra loose folders to take setup files from, highest last.
    pub from: Vec<PathBuf>,
    pub viewport: Option<ViewportMode>,
    pub zoom: Option<f32>,
}

/// A mod folder name: ASCII letters, digits, `-`, `_` and `.`, not
/// starting with a dot.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Export the setup the settings describe as a mod folder; returns it and
/// how many files it carries. The settings' enhanced settings, with the
/// enabled mods' manifests over them and the request's flags over both,
/// fill the manifest; the setup files ([`vault_core::mod_export`]) come
/// from the settings' game data and mod layers, then each `from` folder
/// (a later source wins a path). Nothing is discovered or saved.
pub fn export_setup(
    config: Option<&Config>,
    request: &ExportRequest,
) -> crate::AppResult<(PathBuf, usize)> {
    if !valid_name(&request.name) {
        return Err(format!(
            "mod name {:?}: use letters, digits, '-', '_' and '.'",
            request.name
        )
        .into());
    }
    let none = Config::default();
    let (config, _) = with_mods(config.unwrap_or(&none))?;
    let mut settings = config.enhanced_settings();
    settings.viewport = request.viewport.unwrap_or(settings.viewport);
    settings.default_zoom = request.zoom.unwrap_or(settings.default_zoom);
    let manifest = setup_manifest(
        &request.name,
        "Enhanced viewport, camera and roof-peek settings (with any table overrides and \
         enhanced scripts in use), exported with cargo xtask export-mod.",
        &settings,
        f64::from(settings.default_zoom),
    );
    let mut sources: Vec<Arc<dyn Vfs>> = Vec::new();
    if config.has_game_data() {
        let args = crate::preview::PreviewArgs {
            master_dat: config.master_dat.clone(),
            critter_dat: config.critter_dat.clone(),
            corpus_dir: config.data_root.clone(),
            mods: config.mod_list.clone(),
            ..Default::default()
        };
        sources.push(args.source()?);
    } else if let Some(mods) = &config.mod_list {
        let empty: Arc<dyn Vfs> = Arc::new(crate::launcher::Empty);
        sources.push(crate::preview::PreviewArgs::mount_mods(empty, mods)?);
    }
    for folder in &request.from {
        let vfs = vault_formats::LocalDirProvider::mount(folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
        sources.push(Arc::new(vfs));
    }
    let mut setup = Setup::default();
    for source in &sources {
        setup.merge(vault_core::mod_export::setup(source.as_ref())?);
    }
    let output = request
        .output
        .clone()
        .unwrap_or_else(|| export_root(config.mod_list.as_ref()).join(&request.name));
    let files = write_setup(&output, manifest, setup)?;
    Ok((output, files))
}

/// Write `setup` as a mod folder at `output` with `manifest`, which gains
/// the setup's registrations; returns how many game files it holds.
pub fn write_setup(
    output: &Path,
    mut manifest: ModManifest,
    setup: Setup,
) -> crate::AppResult<usize> {
    manifest.scripts = setup.scripts.into_values().collect();
    manifest.maps = setup.maps.into_values().collect();
    manifest.patches = setup.patches;
    let files: Vec<(String, Vec<u8>)> = setup.files.into_iter().collect();
    mod_manifest::write_mod_folder(output, &manifest, &files)
        .map_err(|error| format!("{}: {error}", output.display()))?;
    Ok(files.len())
}

/// The first free folder `root/<slug>`, `root/<slug>-2`, ... (an export
/// never overwrites).
pub fn free_folder(root: &Path, slug: &str) -> PathBuf {
    (1..)
        .map(|number| match number {
            1 => root.join(slug),
            _ => root.join(format!("{slug}-{number}")),
        })
        .find(|path| !path.exists())
        .expect("an unused folder name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enhanced::{RoofPeek, ViewportMode};

    fn manifest(dir: &Path, text: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(MANIFEST), text).unwrap();
    }

    #[test]
    fn enabled_layers_apply_their_enhanced_settings_highest_last() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        manifest(
            &root.join("mods/camera"),
            "name = \"Camera\"\n[enhanced]\npreset = \"enhanced\"\ndefault_zoom = 2.0\n\
             roof_peek = \"cursor_xray\"\nviewport_mode = \"unlocked_widescreen\"",
        );
        manifest(
            &root.join("mods/calm"),
            "name = \"Calm\"\n[enhanced]\nsmooth_camera = false\ndefault_zoom = 1.5",
        );
        manifest(&root.join("mods/maps"), "name = \"Maps only\"");
        manifest(
            &root.join("mods/off"),
            "name = \"Off\"\n[enhanced]\nroof_peek = \"hidden\"",
        );
        std::fs::write(
            root.join("load_order.toml"),
            "[[layer]]\npath = \"mods/camera\"\n[[layer]]\npath = \"mods/maps\"\n\
             [[layer]]\npath = \"mods/calm\"\n[[layer]]\npath = \"mods/off\"\nenabled = false\n\
             [[layer]]\npath = \"mods/none\"\n[[layer]]\npath = \"patch.dat\"\n",
        )
        .unwrap();
        let config = Config {
            preset: Some(Preset::Vanilla),
            roof_peek: Some(RoofPeek::Vanilla),
            smooth_camera: Some(true),
            mod_list: Some(ModList::File(root.join("load_order.toml"))),
            ..Config::default()
        };
        let (merged, applied) = with_mods(&config).unwrap();
        assert_eq!(applied, ["Camera", "Calm"]);
        assert_eq!(merged.preset, Some(Preset::Enhanced));
        assert_eq!(merged.viewport_mode, Some(ViewportMode::Unlocked));
        assert_eq!(merged.default_zoom, Some(1.5), "the higher layer wins");
        assert_eq!(merged.smooth_camera, Some(false));
        assert_eq!(
            merged.roof_peek,
            Some(RoofPeek::CursorXray),
            "a disabled layer applies nothing"
        );
        // Without a mod list nothing changes.
        let plain = Config::default();
        assert_eq!(with_mods(&plain).unwrap(), (plain, Vec::new()));
        // A malformed manifest stops the launch, naming the folder.
        manifest(
            &root.join("mods/maps"),
            "name = \"Maps\"\n[enhanced]\nzoom = 2",
        );
        let error = with_mods(&config).unwrap_err();
        assert!(error.contains("maps") && error.contains("zoom"), "{error}");
    }

    #[test]
    fn layer_folders_resolve_as_the_mount_resolves_them() {
        let entry = |path: &str| LayerEntry {
            path: Some(path.into()),
            ..LayerEntry::default()
        };
        let strict = LoadOrderPolicy::default();
        let base = Path::new("/games/fallout");
        assert_eq!(
            layer_dir(base, &entry("mods/a"), strict),
            Some(base.join("mods/a"))
        );
        for refused in ["../elsewhere", "/abs/mod", "C:\\mod", "patch.DAT", ""] {
            assert_eq!(layer_dir(base, &entry(refused), strict), None, "{refused}");
        }
        let open = LoadOrderPolicy {
            allow_outside: true,
        };
        assert!(layer_dir(base, &entry("../elsewhere"), open).is_some());
        let paths = ModList::Paths(vec!["/m/a".into(), "/m/b.dat".into()]);
        assert_eq!(layer_dirs(&paths), [PathBuf::from("/m/a")]);
        assert!(layer_dirs(&ModList::File("/missing/load_order.toml".into())).is_empty());
    }

    #[test]
    fn the_settings_export_carries_their_settings_mods_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // A mod layer with a table override and a manifest, and the camera
        // demo corpus as an extra folder.
        let layer = root.join("mods/tables");
        std::fs::create_dir_all(layer.join("data/vault-engine")).unwrap();
        let towns = vault_core::tables::Table::Towns.canonical();
        std::fs::write(layer.join("data/vault-engine/towns.txt"), towns).unwrap();
        manifest(
            &layer,
            "name = \"Tables\"\n[enhanced]\nroof_peek = \"cursor_xray\"",
        );
        std::fs::write(root.join("load_order.toml"), "layers = [\"mods/tables\"]").unwrap();
        let demo = root.join("demo");
        std::fs::create_dir_all(&demo).unwrap();
        vault_fixtures::state::write_corpus(&demo);
        vault_fixtures::state::write_cam_demo(&demo);
        let config = Config {
            smooth_camera: Some(false),
            default_zoom: Some(1.25),
            mod_list: Some(ModList::File(root.join("load_order.toml"))),
            ..Config::default()
        };
        let request = ExportRequest {
            name: "my-camera".into(),
            from: vec![demo],
            zoom: Some(2.0),
            ..ExportRequest::default()
        };
        let (folder, files) = export_setup(Some(&config), &request).unwrap();
        assert_eq!(folder, root.join("mods/my-camera"));
        assert_eq!(files, 2);
        let written = read_manifest(&folder).unwrap().unwrap();
        assert_eq!(written.name, "my-camera");
        let enhanced = written.enhanced.unwrap();
        assert_eq!(enhanced.preset.as_deref(), Some("enhanced"));
        assert_eq!(enhanced.default_zoom, Some(2.0), "the flag wins");
        assert_eq!(enhanced.smooth_camera, Some(false), "the file's");
        assert_eq!(
            enhanced.roof_peek.as_deref(),
            Some("cursor_xray"),
            "the mod's"
        );
        assert_eq!(
            std::fs::read_to_string(folder.join("data/vault-engine/towns.txt")).unwrap(),
            towns
        );
        assert!(folder.join("scripts/camdemo.int").is_file());
        assert!(folder.join("README.txt").is_file());
        // Never over an earlier export, and only plain names.
        assert!(export_setup(Some(&config), &request).is_err());
        for bad in ["", ".hidden", "../up", "a/b", "sp ace"] {
            let request = ExportRequest {
                name: bad.into(),
                output: Some(root.join("other")),
                ..ExportRequest::default()
            };
            assert!(export_setup(None, &request).is_err(), "{bad}");
        }
        assert!(!root.join("other").exists());
        // With no settings at all, the defaults are exported.
        let request = ExportRequest {
            name: "defaults".into(),
            output: Some(root.join("defaults")),
            ..ExportRequest::default()
        };
        assert_eq!(export_setup(None, &request).unwrap().1, 0);
    }

    #[test]
    fn exports_go_beside_the_load_order_in_fresh_folders() {
        let file = ModList::File("/games/fallout/load_order.toml".into());
        assert_eq!(export_root(Some(&file)), Path::new("/games/fallout/mods"));
        assert_eq!(export_root(None), Path::new("mods"));
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(free_folder(dir.path(), "setup"), dir.path().join("setup"));
        std::fs::create_dir(dir.path().join("setup")).unwrap();
        std::fs::create_dir(dir.path().join("setup-2")).unwrap();
        assert_eq!(free_folder(dir.path(), "setup"), dir.path().join("setup-3"));
    }
}
