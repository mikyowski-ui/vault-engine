//! Mod layers' `vault-mod.toml` manifests on the desktop: which enabled
//! folder layers carry one, applying their `[enhanced]` settings to a
//! launch, and where the F9 overlay exports the current setup as a mod.
//! Engine extension, not part of any specification.
use crate::config::{self, Config, ModList, Preset};
use crate::enhanced::{EnhancedSettings, ViewportMode};
use crate::storage::{self, FsStorage, Namespace, StorageBackend, StorageOp};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use vault_core::{
    host_mods::{ModSession, ScriptedMod, script_value},
    mod_export::Setup,
    sfall::SfallValue,
};
use vault_formats::{
    EnhancedOverrides, LayerEntry, LoadOrderPolicy, ModManifest, Vfs,
    mod_manifest::{self, MANIFEST, ModSetting},
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
    Ok(read_manifest_bytes(dir)?.map(|(manifest, _)| manifest))
}

/// [`read_manifest`], with the file's bytes (SPEC-024a §2.3 hashes them).
fn read_manifest_bytes(dir: &Path) -> Result<Option<(ModManifest, Vec<u8>)>, String> {
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
    let text =
        String::from_utf8(bytes.clone()).map_err(|error| format!("{}: {error}", path.display()))?;
    ModManifest::parse(&text)
        .map(|manifest| Some((manifest, bytes)))
        .map_err(|error| format!("{}: {error}", dir.display()))
}

/// One enabled layer with a manifest.
pub struct ManifestLayer {
    dir: PathBuf,
    manifest: ModManifest,
    bytes: Vec<u8>,
}

/// The manifests of the enabled `dirs`, lowest first, under SPEC-024a §2.2's
/// rules that reach past one manifest: each script is in its own layer's
/// `scripts/`, and two layers share no `id` and no script. The error names
/// the folder, and both folders for a conflict.
pub fn manifest_layers(dirs: &[PathBuf]) -> Result<Vec<ManifestLayer>, String> {
    let mut layers: Vec<ManifestLayer> = Vec::new();
    for dir in dirs {
        let Some((manifest, bytes)) = read_manifest_bytes(dir)? else {
            continue;
        };
        let scripts = script_files(&manifest);
        for file in &scripts {
            let path = format!("scripts/{file}");
            if crate::launcher::validate::find_loose(dir, &path).is_none() {
                return Err(format!("{}: {path} is not in this layer", dir.display()));
            }
        }
        for other in &layers {
            if let (Some(id), Some(theirs)) = (&manifest.id, &other.manifest.id)
                && id == theirs
            {
                return Err(format!(
                    "{} and {} both have id \"{id}\"",
                    other.dir.display(),
                    dir.display()
                ));
            }
            let named = script_files(&other.manifest);
            if let Some(file) = scripts.iter().find(|file| named.contains(*file)) {
                return Err(format!(
                    "{} and {} both name {file}",
                    other.dir.display(),
                    dir.display()
                ));
            }
        }
        layers.push(ManifestLayer {
            dir: dir.clone(),
            manifest,
            bytes,
        });
    }
    Ok(layers)
}

/// The scripts `manifest`'s rows name, lowercased.
fn script_files(manifest: &ModManifest) -> Vec<String> {
    let globals = manifest.global_scripts.iter().map(|row| row.file.clone());
    let hooks = manifest.hooks.iter().map(|row| row.file.clone());
    let mut files: Vec<String> = globals.chain(hooks).collect();
    files.sort_unstable();
    files.dedup();
    files
}

/// A script's value as a setting, for storage.
fn stored_value(value: &SfallValue) -> ModSetting {
    match value {
        SfallValue::Int(value) => ModSetting::Int(*value),
        SfallValue::Float(bits) => ModSetting::Float(f32::from_bits(*bits)),
        SfallValue::Text(text) => ModSetting::Text(String::from_utf8_lossy(text).into_owned()),
    }
}

/// SPEC-024a §2.3's text of a setting: `i:` and decimal, `f:` and the
/// binary32 bits, or `s:` and the text.
pub fn record_value(value: &SfallValue) -> String {
    match value {
        SfallValue::Int(value) => format!("i:{value}"),
        SfallValue::Float(bits) => format!("f:{bits:08X}"),
        SfallValue::Text(text) => format!("s:{}", String::from_utf8_lossy(text)),
    }
}

/// [`record_value`] read back.
pub fn parse_record_value(text: &str) -> Option<SfallValue> {
    let (kind, value) = text.split_once(':')?;
    match kind {
        "i" => value.parse().ok().map(SfallValue::Int),
        "f" => u32::from_str_radix(value, 16)
            .ok()
            .map(|bits| SfallValue::float(f32::from_bits(bits))),
        "s" => Some(SfallValue::text(value.as_bytes())),
        _ => None,
    }
}

/// The `config` key of mod `id`'s changed settings (SPEC-024a §7.4).
fn settings_key(id: &str) -> String {
    format!("mod-settings-{id}.toml")
}

/// The stored settings of a mod over its manifest's: a key the manifest does
/// not declare, or a value of the wrong type, is dropped with a diagnostic.
fn apply_stored(session: &ModSession, index: usize, storage: &dyn StorageBackend) {
    let id = &session.mods()[index].id;
    let key = settings_key(id);
    let text = match storage.read(Namespace::Config, &key) {
        Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
        Ok(None) => return,
        Err(error) => {
            session.note(format!("{key}: {error}"));
            return;
        }
    };
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let restored = line.split_once('=').is_some_and(|(name, value)| {
            ModSetting::parse(value)
                .is_ok_and(|setting| session.restore(index, name.trim(), script_value(&setting)))
        });
        if !restored {
            session.note(format!("{key}: dropped {line:?}"));
        }
    }
}

/// The session's scripted mods, when any enabled layer of `dirs` is scripted
/// (SPEC-024a §2.1). Settings start from `storage`; a replay passes none, and
/// `commits` is off, because it takes them from its log and writes nothing.
pub fn mod_session(
    dirs: &[PathBuf],
    storage: Option<&dyn StorageBackend>,
    commits: bool,
) -> Result<Option<Arc<ModSession>>, String> {
    let mods: Vec<ScriptedMod> = manifest_layers(dirs)?
        .iter()
        .filter(|layer| layer.manifest.scripted())
        .map(scripted_mod)
        .collect();
    if mods.is_empty() {
        return Ok(None);
    }
    let session = ModSession::new(mods, commits);
    if let Some(storage) = storage {
        for index in 0..session.mods().len() {
            apply_stored(&session, index, storage);
        }
    }
    Ok(Some(Arc::new(session)))
}

fn scripted_mod(layer: &ManifestLayer) -> ScriptedMod {
    ScriptedMod::from_manifest(&layer.manifest, &layer.bytes)
}

/// The batch of SPEC-024a §7.4's commit: each changed mod's file, holding
/// the keys that differ from its manifest, or no file when none does.
pub fn settings_batch(changed: Vec<(String, BTreeMap<String, SfallValue>)>) -> Vec<StorageOp> {
    changed
        .into_iter()
        .map(|(id, keys)| {
            let key = settings_key(&id);
            if keys.is_empty() {
                return StorageOp::Delete { key };
            }
            let text: String = keys
                .iter()
                .map(|(name, value)| format!("{name} = {}\n", stored_value(value).to_toml()))
                .collect();
            StorageOp::put(key, text)
        })
        .collect()
}

/// `manifest`'s `[enhanced]` settings over `config` (each key it sets
/// replaces the settings file's).
pub fn apply(config: &mut Config, manifest: &ModManifest) {
    if let Some(sfall) = manifest.sfall {
        config.sfall = Some(sfall);
    }
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
            if manifest.enhanced.is_some() || manifest.sfall.is_some() {
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

    #[test]
    fn the_highest_layer_that_sets_sfall_wins_over_the_settings_file() {
        let mut config = Config::default();
        let on = ModManifest::parse("name = \"On\"\n[scripting]\nsfall = true").unwrap();
        let off = ModManifest::parse("name = \"Off\"\n[scripting]\nsfall = false").unwrap();
        let silent = ModManifest::parse("name = \"Silent\"").unwrap();
        apply(&mut config, &on);
        assert_eq!(config.sfall, Some(true));
        apply(&mut config, &silent);
        assert_eq!(config.sfall, Some(true));
        apply(&mut config, &off);
        assert_eq!(config.sfall, Some(false));
    }

    const N1: &str = include_str!("../../../tests/fixtures/spec024a/vault-mod.toml");

    fn scripted_layer(dir: &Path, manifest_text: &str, script: Option<&str>) {
        manifest(dir, manifest_text);
        if let Some(file) = script {
            std::fs::create_dir_all(dir.join("scripts")).unwrap();
            std::fs::write(dir.join("scripts").join(file), b"").unwrap();
        }
    }

    /// SPEC-024a §9.5's vectors that need folders: N8, N10, N11 and N12.
    #[test]
    fn spec024a_layers_are_checked_across_the_load_order() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        scripted_layer(&a, N1, None);
        let error = manifest_layers(std::slice::from_ref(&a)).err().unwrap();
        assert!(error.contains("scripts/gl_cheapap.int"), "N8: {error}");
        scripted_layer(&a, N1, Some("GL_CHEAPAP.INT"));
        assert_eq!(
            manifest_layers(std::slice::from_ref(&a))
                .ok()
                .unwrap()
                .len(),
            1
        );
        scripted_layer(&b, N1, Some("gl_cheapap.int"));
        let both = [a.clone(), b.clone()];
        let error = manifest_layers(&both).err().unwrap();
        assert!(
            error.contains(&a.display().to_string()) && error.contains(&b.display().to_string()),
            "N10: {error}"
        );
        let renamed = N1.replace("cheap-ap", "other");
        scripted_layer(&b, &renamed, Some("gl_cheapap.int"));
        let error = manifest_layers(&both).err().unwrap();
        assert!(error.contains("both name gl_cheapap.int"), "{error}");
        // N11: a disabled layer counts for nothing.
        std::fs::write(
            dir.path().join("load_order.toml"),
            "[[layer]]\npath = \"a\"\nenabled = false\n",
        )
        .unwrap();
        let list = ModList::File(dir.path().join("load_order.toml"));
        assert!(
            mod_session(&layer_dirs(&list), None, true)
                .unwrap()
                .is_none()
        );
        // N12: settings alone are valid, and not scripted.
        let n12: String = N1.split("[[global_scripts]]").next().unwrap().to_owned();
        scripted_layer(&a, &n12, None);
        let found = mod_session(std::slice::from_ref(&a), None, true).unwrap();
        assert!(found.is_none(), "N12");
    }

    /// §2.2: the launcher shows the launch's error on the layer.
    #[test]
    fn spec024a_the_launcher_shows_a_conflict_on_the_layer() {
        let dir = tempfile::tempdir().unwrap();
        scripted_layer(&dir.path().join("a"), N1, Some("gl_cheapap.int"));
        scripted_layer(&dir.path().join("b"), N1, Some("gl_cheapap.int"));
        let entries =
            vault_formats::parse_load_order("[[layer]]\npath = \"a\"\n[[layer]]\npath = \"b\"\n")
                .unwrap();
        let checked =
            crate::launcher::validate::mods(dir.path(), &entries, LoadOrderPolicy::default());
        assert!(checked.layers[0].is_ok());
        assert!(!checked.layers[1].is_ok());
    }

    /// §2.3's text of each type of setting.
    #[test]
    fn spec024a_setting_records_round_trip() {
        for (value, text) in [
            (SfallValue::Int(-3), "i:-3"),
            (SfallValue::float(1.5), "f:3FC00000"),
            (SfallValue::text(&b"a:b"[..]), "s:a:b"),
        ] {
            assert_eq!(record_value(&value), text);
            assert_eq!(parse_record_value(text), Some(value));
        }
        assert_eq!(parse_record_value("x:1"), None);
        let keys = BTreeMap::from([("b".to_owned(), SfallValue::Int(2))]);
        let keys = [("m".to_owned(), keys), ("n".to_owned(), BTreeMap::new())];
        assert_eq!(
            settings_batch(keys.to_vec()),
            [
                StorageOp::put("mod-settings-m.toml", "b = 2\n"),
                StorageOp::Delete {
                    key: "mod-settings-n.toml".into()
                },
            ]
        );
    }
}
