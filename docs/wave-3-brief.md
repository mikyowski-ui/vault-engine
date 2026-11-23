# Wave 3 brief: the launcher studio

The owner's brief of 2026-11-01 is the authority. This note is the contract the
concurrent agents share. The clean-room wall is unchanged.

## Settings

`crates/vault-platform/src/config.rs` already holds the schema. Nobody but the lead
edits it during the batch. Ask for a change in your report.

- `ProfileScope` carries `sfall`, `mod_list`, `install_dir`, `secondary_dat_dirs` and
  `enhanced: EnhancedProfile`. `Config` holds the active scope's copy of each at the
  top level, as it does `mod_list`. `Config::scoped_to` lays `enhanced` over the
  top-level settings for a launch.
- Profiles: `profile_names`, `active_profile`, `select_profile`, `add_custom_profile`.
- Presets: `custom_presets`, `save_custom_preset`.
- `known_paths: KnownPaths` with `remember`, `forget`, `list`, and `KnownKind`.
- `entry_any_case(dir, name, keep)` is the one case-insensitive lookup.
- `save_profile_enhanced(path, name, settings)` writes that profile's table alone.

## Launcher

`launcher/mod.rs` declares `Tab`, `Widget`, `LauncherAction` and `ProfileCard`. The
owner of that file fills in their behaviour. The owner of `view.rs` draws them and
maps pixels to widgets. Add variants if you need them, and do not rename one. The
`Paths & Mods` pill shows `Tab::Data` and `Tab::Mods` together, `Enhanced & Visuals`
is `Tab::Display`, then `Tab::Audio` and `Tab::Debug`.

## Preview

`gallery/settings_preview.rs` is drawn procedurally. A shipped launcher has no game
data, and `vault-fixtures` is a dev dependency, so it cannot run a session. It takes
`PreviewSettings` and draws into a rectangle in the launcher's palette.

## Et Tu detection

Detection looks at names only: a folder or file called `fallout1in2`, `mods` or
`ddraw.ini`. Nothing reads the contents of a mod, `ddraw.ini`, `sfall.dat`, `ddraw.dll`
or `f2_res.*`. Tests use synthetic folders. No agent opens a real install.

## Icon

`crates/vault-platform/assets/icon.rs` is plain Rust with no dependencies, so the
platform and `xtask` can both include it by `#[path]`, as `vault-fixtures` does.
