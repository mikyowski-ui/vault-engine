use std::path::PathBuf;

use clap::Parser;
use vault_render::ScalingMode;
use winit::event_loop::{ControlFlow, EventLoop};

use super::{DisplayPalette, InputLog, Shell, session_toasts};
#[cfg(test)]
use super::{boot_flow::apply_choice, *};
use crate::{
    AppResult, audio, autoplay_runner, boot, config, enhanced, gallery, launcher, mods, preview,
    replay, session,
};

#[derive(Parser, Clone)]
#[command(
    about = "Vault Engine desktop shell: sprite preview and the interactive world, from loose files or DAT archives",
    long_about = "Vault Engine desktop shell: sprite preview and the interactive world, from loose files or DAT archives.\n\nWithout arguments the game starts from the data named in vault-engine.toml (beside the executable, else in the working directory). Without that file, each game's MASTER.DAT and CRITTER.DAT are looked for in the working directory and beside the executable, then Fallout 1's in ../local-corpus, ~/games/fallout1 and its Steam and GOG folders, and Fallout 2's in ~/games/fallout2 and its Steam and GOG folders; what is found is saved to vault-engine.toml. With nothing valid, the launcher opens. Command-line flags always win over the file.\n\nF11 or Alt+Enter toggles borderless fullscreen; Alt+S toggles aspect-fit and integer scaling; with the enhanced preset, Alt+V toggles the fixed and unlocked viewports, Alt+F cycles the game speed (1.0x, 1.5x, 2.0x) and Alt+C toggles the camera follow (off, smooth). Home centers the view on the player."
)]
pub(crate) struct Args {
    /// Present one frame and exit successfully (requires a desktop and GPU backend).
    #[arg(long)]
    smoke_test: bool,
    /// Do not open an audio output device; sounds resolve but stay silent.
    #[arg(long)]
    no_audio: bool,
    /// Replay a captured JSONL input log headlessly (no window, GPU, or
    /// audio) against the selected preview, ticking it at the logged frame
    /// times, then print the final state SHA-256 digest as JSON.
    #[arg(
        long,
        value_name = "PATH",
        requires = "preview_asset",
        conflicts_with_all = ["smoke_test", "no_audio"]
    )]
    replay_inputs: Option<PathBuf>,
    /// Play the named map with the autoplay bot, with no window, GPU, audio or
    /// sleeping, auditing saves as it goes. A run that finds a failure writes
    /// a bundle to --autoplay-output and exits with status 1.
    #[arg(
        long,
        requires = "preview_asset",
        conflicts_with_all = ["replay_inputs", "smoke_test", "autoplay_watch", "replay_bundle"]
    )]
    autoplay_headless: bool,
    /// Run this settings profile (`[profiles.NAME]`): its data, mods and
    /// Enhanced settings. With `--game` it starts the game; a profile the
    /// settings do not have opens the launcher, saying so. With
    /// `--autoplay-headless` it gives the run its data, linked data, mods and
    /// sfall, under Vanilla rules; an unknown name is an error.
    #[arg(long, value_name = "NAME", conflicts_with_all = ["replay_inputs", "game_profile"])]
    profile: Option<String>,
    /// Let the autoplay bot play the visible game, with the Watch Mode banner.
    /// Space hands control to the player.
    #[arg(long, conflicts_with_all = ["replay_inputs", "smoke_test"])]
    autoplay_watch: bool,
    /// With --autoplay-headless: the most ticks to run.
    #[arg(long, value_name = "N", default_value_t = 50_000)]
    ticks: u64,
    /// With the autoplay flags: audit doors, locks and explosives on each map.
    #[arg(long)]
    chaos_doors_c4: bool,
    /// With --autoplay-headless: the folder for the run's log, checkpoints,
    /// manifest and report.
    #[arg(long, value_name = "DIR", default_value = "autoplay-run")]
    autoplay_output: PathBuf,
    /// Replay a bundle --autoplay-headless wrote, comparing its checkpoints.
    #[arg(long, value_name = "DIR", conflicts_with_all = ["replay_inputs", "smoke_test"])]
    replay_bundle: Option<PathBuf>,
    /// SPEC-024a §2.3: a load order, as a `mod_list` file. It wins over the
    /// settings file, and it is how a replay mounts layers.
    #[arg(long, value_name = "PATH")]
    mod_list: Option<PathBuf>,
    /// With --replay-inputs: also write the final frame as a binary PPM.
    #[arg(long, value_name = "PATH", requires = "replay_inputs")]
    replay_frame: Option<PathBuf>,
    /// With --replay-inputs: write the SPEC-021 checkpoint stream, one row
    /// per step, to FILE (created or replaced). Changes no digest.
    #[arg(long, value_name = "FILE", requires = "replay_inputs")]
    checkpoints_out: Option<PathBuf>,
    /// With --replay-inputs: compare each checkpoint row with the stream in
    /// FILE. The run stops at the first failure, prints the report on
    /// stderr and no digest, and exits with status 2.
    #[arg(long, value_name = "FILE", requires = "replay_inputs")]
    checkpoints_expect: Option<PathBuf>,
    /// Record every input command and frame clock as a JSONL input log to
    /// FILE (`-`: standard output), for --replay-inputs. Without it no
    /// input log is written.
    #[arg(long, value_name = "FILE", conflicts_with = "replay_inputs")]
    record_inputs: Option<PathBuf>,
    /// Export the current setup as a mod folder named NAME and exit
    /// (`cargo xtask export-mod` runs this): a `vault-mod.toml` of the
    /// settings file's enhanced settings (the enabled mods' manifests and
    /// `--viewport`/`--zoom` over them), the table overrides and enhanced
    /// scripts of its game data and mods (and of each `--export-from`
    /// folder), and a README.txt. Nothing is discovered or saved.
    #[arg(
        long,
        value_name = "NAME",
        conflicts_with_all = ["replay_inputs", "smoke_test", "launcher", "preview_asset"]
    )]
    export_mod: Option<String>,
    /// Write the documentation gallery into DIR and exit (`cargo xtask
    /// capture-gallery` runs this, with the inputs it stages).
    #[arg(
        long,
        hide = true,
        value_name = "DIR",
        requires = "capture_stage",
        conflicts_with_all = ["replay_inputs", "smoke_test", "launcher", "export_mod"]
    )]
    capture_gallery: Option<PathBuf>,
    /// The folder `--capture-gallery` reads its synthetic inputs from.
    #[arg(long, hide = true, value_name = "DIR", requires = "capture_gallery")]
    capture_stage: Option<PathBuf>,
    /// The folder `--export-mod` writes; it must not exist or be empty.
    /// Default: mods/NAME beside the load order, else in the working
    /// directory.
    #[arg(long, value_name = "PATH", requires = "export_mod")]
    export_output: Option<PathBuf>,
    /// A loose folder `--export-mod` also takes table overrides and
    /// enhanced scripts from (repeatable; a later one wins a file), such
    /// as the camera demo corpus `cargo xtask fixtures` writes.
    #[arg(long, value_name = "PATH", requires = "export_mod")]
    export_from: Vec<PathBuf>,
    /// How the frame is scaled into the window: `aspect_fit` (the largest
    /// fit, sharp-bilinear at fractional scales) or `integer` (whole scales
    /// only). Default: the settings file's, else integer. Alt+S toggles it.
    #[arg(long, value_name = "MODE", value_parser = scaling_arg)]
    scaling: Option<ScalingMode>,
    /// Start in borderless fullscreen (F11 or Alt+Enter toggles it).
    #[arg(long)]
    fullscreen: bool,
    /// Turn on the touch UI: taps click, a drag pans and a pinch zooms the
    /// camera (under Vanilla too), a hold opens the action menu, a
    /// two-finger tap right-clicks, and an on-screen pill bar has MODE,
    /// PEEK, TURN, INV, PIP and ESC. Default: the settings file's
    /// `touch_ui`, else off.
    #[arg(long)]
    touch_ui: bool,
    /// The engine preset: `vanilla` or `enhanced`. Default: the settings
    /// file's, else vanilla.
    #[arg(long, value_name = "PRESET", value_parser = preset_arg)]
    pub(super) preset: Option<config::Preset>,
    /// The game whose data this is: `fallout1` or `fallout2`. Default: the
    /// settings file's `game`, else fallout1. Any other name is refused, and
    /// so is a session whose MASTER.DAT is the other game's archive.
    #[arg(long, value_name = "GAME", value_parser = game_arg)]
    game_profile: Option<vault_core::GameProfile>,
    /// With the enhanced preset, the viewport: `fixed_640x480` or
    /// `unlocked_widescreen` (the world across the window behind the
    /// interface). Default: the settings file's `viewport_mode`, else
    /// unlocked. Alt+V toggles it.
    #[arg(long, value_name = "MODE", value_parser = viewport_arg)]
    viewport: Option<enhanced::ViewportMode>,
    /// With the enhanced preset, the zoom a map starts at, 0.5 to 3.0.
    /// Default: the settings file's `default_zoom`, else 1.0.
    #[arg(long, value_name = "ZOOM", value_parser = zoom_arg)]
    zoom: Option<f32>,
    /// With the enhanced preset, how the view follows the player: `off`
    /// (it stays put until you scroll, press Home, or the player nears its
    /// edge), `step` (it recentres at every hex step, as Vanilla does) or
    /// `smooth` (it tracks the player every frame). Default: the settings
    /// file's `camera_follow`, else off. Alt+C toggles off and smooth in
    /// play. The Vanilla preset always uses step: another value is refused
    /// there, with a warning, and ignored.
    #[arg(long, value_name = "MODE", value_parser = camera_follow_arg)]
    camera_follow: Option<enhanced::CameraFollow>,
    /// How the world canvas is drawn (SPEC-022): `reference`, the CPU
    /// compositor, or `instanced`, GPU-instanced sprite batches that draw
    /// the same pixels and fall back to the reference for any refresh they
    /// cannot draw exactly (or for the whole run without a GPU). Any preset
    /// and game, replays and `--capture-gallery` included; never logged or
    /// saved. Default: the settings file's `render_path`, else reference.
    /// The F9 overlay's renderer page shows the path and its counts.
    #[arg(long, value_name = "PATH", value_parser = render_path_arg)]
    render_path: Option<vault_render::RenderPath>,
    /// Open the launcher (game data, display, audio and mod settings, then
    /// Play) even when the settings are complete. A start naming nothing
    /// opens it anyway while the settings name no valid game data.
    #[arg(long, conflicts_with_all = ["replay_inputs", "smoke_test", "preview_asset"])]
    launcher: bool,
    #[command(flatten)]
    pub(super) preview: preview::PreviewArgs,
}

fn scaling_arg(text: &str) -> Result<ScalingMode, String> {
    config::parse_scaling(text).ok_or_else(|| "expected aspect_fit or integer".into())
}
fn game_arg(text: &str) -> Result<vault_core::GameProfile, String> {
    session::parse_edition(text).map_err(|error| error.to_string())
}
fn preset_arg(text: &str) -> Result<config::Preset, String> {
    config::Preset::parse(text).ok_or_else(|| "expected vanilla or enhanced".into())
}
fn viewport_arg(text: &str) -> Result<enhanced::ViewportMode, String> {
    config::parse_viewport(text)
        .ok_or_else(|| "expected fixed_640x480 or unlocked_widescreen".into())
}
fn render_path_arg(text: &str) -> Result<vault_render::RenderPath, String> {
    vault_render::RenderPath::parse(text).ok_or_else(|| "expected reference or instanced".into())
}
fn zoom_arg(text: &str) -> Result<f32, String> {
    config::parse_zoom(text).ok_or_else(|| "expected a number from 0.5 to 3.0".into())
}
fn camera_follow_arg(text: &str) -> Result<enhanced::CameraFollow, String> {
    enhanced::CameraFollow::parse(text).ok_or_else(|| "expected off, step or smooth".into())
}

/// The diagnostic for a `--camera-follow` the Vanilla preset refuses: any
/// value but `step`, which is what Vanilla always does. `None` when there
/// is nothing to refuse.
fn refused_camera_follow(
    preset: config::Preset,
    mode: Option<enhanced::CameraFollow>,
) -> Option<String> {
    let mode = mode.filter(|mode| {
        preset == config::Preset::Vanilla && *mode != enhanced::CameraFollow::Step
    })?;
    Some(format!(
        "--camera-follow {}: the Vanilla preset recentres the view at every step; ignored",
        mode.as_str()
    ))
}

impl Args {
    /// An asset to preview, or the game, is named.
    fn names_asset(&self) -> bool {
        let preview = &self.preview;
        preview.game
            || preview.preview_frm.is_some()
            || preview.preview_map.is_some()
            || preview.play_movie.is_some()
    }
    /// SPEC-020 §2.4: a start that the boot selector may answer. Display
    /// flags do not stop it from being one, and no replay, smoke test or
    /// named game or asset ever is.
    fn zero_argument(&self) -> bool {
        self.game_profile.is_none()
            && self.profile.is_none()
            && !self.launcher
            && !self.smoke_test
            && self.replay_inputs.is_none()
            && !self.names_asset()
            && !self.names_source()
            && self.preview.critter_dat.is_none()
    }
    /// The command line names the asset source.
    fn names_source(&self) -> bool {
        self.preview.corpus_dir.is_some() || self.preview.master_dat.is_some()
    }
    /// The launch would use game data the command line does not name: the
    /// settings file's or, without one, discovered data. A replay never
    /// does, and neither does a bare smoke test (the color test).
    fn wants_data(&self) -> bool {
        self.replay_inputs.is_none()
            && !self.names_source()
            && (self.names_asset() || !self.smoke_test)
    }
}

/// What the command line and the settings file make of a launch.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Launch {
    pub(super) scaling: ScalingMode,
    pub(super) fullscreen: bool,
    /// The touch UI (gestures and the pill bar).
    pub(super) touch_ui: bool,
    game: vault_core::GameProfile,
    preset: config::Preset,
    /// With the enhanced preset only, its presentation settings.
    pub(super) enhanced: Option<enhanced::EnhancedSettings>,
    /// The settings' mod list, for the engine to load.
    mod_list: Option<config::ModList>,
    /// No asset was named and the settings' game data starts the game.
    zero_arg_game: bool,
    /// The settings' `audio = false` keeps the device closed (as
    /// `--no-audio` does).
    audio: bool,
}

/// Merge `config` into the command line, which always wins: with no asset
/// named (and no smoke test) settings that name game data start the game;
/// an asset or the game named without a source takes the settings' data;
/// and the display, preset and debug settings fill what the flags leave
/// unset. A replay takes nothing from the settings: it runs as logged.
fn resolve_launch(args: &mut Args, config: Option<&config::Config>) -> Launch {
    let none = config::Config::default();
    let config = match config {
        Some(config) if args.replay_inputs.is_none() => config,
        _ => &none,
    };
    let named = args.names_asset();
    let game = args.game_profile.or(config.game).unwrap_or_default();
    // The launched game's own paths (its profile when the flag names the
    // other game).
    let paths = config.launch_paths(game);
    let zero_arg_game = args.wants_data() && !named && paths.has_game_data();
    if zero_arg_game {
        args.preview.game = true;
    }
    if (named || zero_arg_game) && args.wants_data() {
        if let (Some(master), Some(critter)) = (paths.master_dat, paths.critter_dat) {
            args.preview.master_dat = Some(master);
            args.preview.critter_dat = Some(critter);
        }
        args.preview.corpus_dir = paths.data_root;
    }
    args.preview.debug |= config.debug.unwrap_or(false);
    let preset = args.preset.or(config.preset).unwrap_or_default();
    let touch_ui = args.touch_ui || config.touch_ui.unwrap_or(false);
    let settings = config.enhanced_settings();
    if let Some(diagnostic) = refused_camera_follow(preset, args.camera_follow) {
        tracing::warn!("{diagnostic}");
    }
    let enhanced = match preset {
        config::Preset::Enhanced => Some(enhanced::EnhancedSettings {
            viewport: args.viewport.unwrap_or(settings.viewport),
            default_zoom: args.zoom.unwrap_or(settings.default_zoom),
            camera_follow: args.camera_follow.unwrap_or(settings.camera_follow),
            ..settings
        }),
        // The touch UI pans and zooms under Vanilla too: the unlocked view
        // with its camera only, the Vanilla rules and interface otherwise.
        config::Preset::Vanilla if touch_ui => Some(enhanced::EnhancedSettings {
            viewport: enhanced::ViewportMode::Unlocked,
            default_zoom: args.zoom.unwrap_or(settings.default_zoom),
            roof_peek: enhanced::RoofPeek::Vanilla,
            navigation_only: true,
            // Vanilla has no follow setting: the touch camera tracks the
            // player as it always has.
            camera_follow: enhanced::CameraFollow::Smooth,
            ..settings
        }),
        config::Preset::Vanilla => None,
    };
    Launch {
        scaling: args
            .scaling
            .or(config.scaling)
            .unwrap_or(ScalingMode::AspectFit),
        fullscreen: args.fullscreen || config.fullscreen.unwrap_or(false),
        touch_ui,
        game,
        preset,
        enhanced,
        mod_list: args
            .mod_list
            .clone()
            .map(config::ModList::File)
            .or_else(|| config.mod_list.clone()),
        zero_arg_game,
        audio: !args.no_audio && config.audio.unwrap_or(true),
    }
}

/// The game speed a launch runs at: the settings' `game_speed` under the
/// enhanced preset, none under Vanilla.
pub(super) fn launch_game_speed(
    launch: &Launch,
    config: Option<&config::Config>,
) -> Option<enhanced::GameSpeed> {
    (launch.preset == config::Preset::Enhanced)
        .then(|| config.map_or_else(Default::default, config::Config::game_speed))
}

/// SPEC-022 §3.1 the render path a launch draws the world canvas with:
/// `--render-path`, else the settings' `render_path`, else the reference
/// path. A replay takes only the flag (it reads no settings).
fn launch_render_path(args: &Args, config: Option<&config::Config>) -> vault_render::RenderPath {
    let config = config.filter(|_| args.replay_inputs.is_none());
    args.render_path
        .or(config.and_then(|config| config.render_path))
        .unwrap_or_default()
}

/// The settings with the enabled mod layers' `vault-mod.toml` `[enhanced]`
/// settings over them ([`mods::with_mods`]), which [`resolve_launch`]
/// then puts under the command line's flags. `None` for a replay, which
/// takes nothing from the settings, or without settings. A malformed
/// manifest stops the launch.
fn with_mod_settings(
    args: &Args,
    config: Option<&config::Config>,
) -> AppResult<Option<config::Config>> {
    let Some(config) = config.filter(|_| args.replay_inputs.is_none()) else {
        return Ok(None);
    };
    let mut config = config.clone();
    if let Some(list) = &args.mod_list {
        config.mod_list = Some(config::ModList::File(list.clone()));
    }
    let (merged, applied) =
        mods::with_mods(&config).map_err(|error| format!("mod_list: {error}"))?;
    if !applied.is_empty() {
        tracing::info!(?applied, "mod settings applied");
    }
    Ok(Some(merged))
}

/// A run starts on a map. `--game` and the other assets satisfy the command
/// line, but name no map for the bot to begin on.
fn check_headless_start(args: &Args) -> AppResult<()> {
    if args.preview.preview_map.is_none() {
        return Err("--autoplay-headless starts on a map: name one with --preview-map".into());
    }
    Ok(())
}

/// Give a headless run the session `--profile` names, as a start of that
/// profile reads it: its edition, data, linked data, mods and sfall. The
/// command line wins as ever, and nothing writes the settings. A name the
/// settings do not have is an error, as a run has no launcher to show it in.
/// The rules stay Vanilla, since a bundle holds no Enhanced settings to
/// replay them with.
fn apply_headless_profile(args: &mut Args, settings: &config::Config) -> AppResult<()> {
    let name = args.profile.as_deref().unwrap_or_default();
    let mut chosen = settings.clone();
    if !chosen.select_profile(name) {
        return Err(format!("no profile named {name} in the settings").into());
    }
    let scoped = chosen.scoped_to(chosen.active_game());
    args.preview.linked_dirs = scoped.secondary_dat_dirs.clone();
    let modded = with_mod_settings(args, Some(&scoped))?;
    let config = modded.as_ref().unwrap_or(&scoped);
    let launch = resolve_launch(args, Some(config));
    args.preview.game_profile = launch.game;
    args.preview.mods = launch.mod_list;
    args.preview.sfall |= config.sfall.unwrap_or(false);
    Ok(())
}

/// Merge the settings into the command line, start the session it names
/// (SPEC-020 §2.3: from the command line or the launcher's Play, which
/// differ only in `origin` and the settings), and open the audio output a
/// world or a movie plays through.
pub(super) fn start(
    args: &mut Args,
    config: Option<&config::Config>,
    origin: session::SessionOrigin,
) -> AppResult<(
    Launch,
    Option<session::SessionRuntime>,
    Option<audio::Audio>,
)> {
    // A named profile is made active for this start, in a copy: nothing here
    // writes the settings.
    let chosen = config.and_then(|config| {
        let name = args.profile.as_deref()?;
        let mut chosen = config.clone();
        chosen.select_profile(name).then_some(chosen)
    });
    let config = chosen.as_ref().or(config);
    // Only the game this start names reads its profile's `sfall` and
    // `mod_list`, however the settings left their top-level values.
    let game = args
        .game_profile
        .or(config.and_then(|config| config.game))
        .unwrap_or_default();
    let scoped = config.map(|config| config.scoped_to(game));
    let config = scoped.as_ref();
    // A custom profile keeps its own saves; `SessionRuntime::start` gives an
    // edition's root the same way when no profile names another.
    if !matches!(
        origin,
        session::SessionOrigin::Replay | session::SessionOrigin::Gallery
    ) && !args.preview.headless
        && args.preview.save_dir.is_none()
        && let Some(config) = config
    {
        args.preview.save_dir = Some(session::desktop_save_root(config.scope_name(game)));
    }
    if let Some(config) = config {
        args.preview.linked_dirs = config.secondary_dat_dirs.clone();
        args.preview.profile_name = Some(config.scope_name(game).to_owned());
    }
    let modded = with_mod_settings(args, config)?;
    let launch = resolve_launch(args, modded.as_ref().or(config));
    if launch.zero_arg_game {
        tracing::info!("starting the game from the settings' data");
    }
    args.preview.preset = match launch.preset {
        config::Preset::Vanilla => vault_core::EnginePreset::Vanilla,
        config::Preset::Enhanced => vault_core::EnginePreset::Enhanced,
    };
    args.preview.game_profile = launch.game;
    args.preview.mods = launch.mod_list.clone();
    args.preview.enhanced = launch.enhanced;
    args.preview.touch_ui = launch.touch_ui;
    args.preview.mod_export = Some(mods::export_root(launch.mod_list.as_ref()));
    args.preview.render_path = launch_render_path(args, config);
    // The backdrop chunk cache's budget; a replay reads no settings.
    args.preview.backdrop_cache_mib = config
        .filter(|_| args.replay_inputs.is_none())
        .and_then(|config| config.backdrop_cache_mib);
    // SPEC-024b §3.1: `--sfall` wins; a replay reads no settings.
    if args.replay_inputs.is_none() {
        args.preview.sfall |= modded
            .as_ref()
            .or(config)
            .and_then(|config| config.sfall)
            .unwrap_or(false);
    }
    let mut session =
        session::SessionRuntime::start(session::SessionConfig::new(origin, args.preview.clone()))?;
    // Audio serves an interactive world or a movie; sprites need none.
    let audio = session
        .as_mut()
        .and_then(|session| session.open_audio(launch.audio));
    Ok((launch, session, audio))
}

/// SPEC-020 §2.5: what a warm switch keeps of a command line. Only flags
/// that a zero-argument start may carry cross it, so `--sfall`,
/// `--master-dat`, `--mod-list` and `--record-inputs` do not.
fn switch_args(shown: &Args) -> Args {
    let mut args = Args::try_parse_from(["vault-platform"]).expect("no flags parse");
    args.scaling = shown.scaling;
    args.fullscreen = shown.fullscreen;
    args.touch_ui = shown.touch_ui;
    args.preset = shown.preset;
    args.viewport = shown.viewport;
    args.zoom = shown.zoom;
    args.camera_follow = shown.camera_follow;
    args.render_path = shown.render_path;
    args.no_audio = shown.no_audio;
    args
}

/// SPEC-020 §2.4 what a start does, and the installs it found: `None` for
/// any start the command line names, which runs exactly as it always has.
fn boot_plan(
    args: &Args,
    config: Option<&config::Config>,
    parses: bool,
    dirs: &config::PerGame<Vec<PathBuf>>,
) -> Option<(boot::Decision, boot::Installs)> {
    if !args.zero_argument() {
        return None;
    }
    let none = config::Config::default();
    let config = config.unwrap_or(&none);
    let found = boot::discover(config, dirs);
    let shown = config::PerGame {
        fallout1: found.fallout1.is_ok(),
        fallout2: found.fallout2.is_ok(),
    };
    let decision = boot::decide(parses, shown, config.last_played.as_deref());
    Some((decision, found))
}

/// The command line the launcher's Play starts from: the one it was shown
/// for, its display flags dropped (the launcher showed them as its own
/// choices and saved what it launches with), debug on for Launch with
/// Debug. With the launcher's settings, [`start`] then builds the
/// zero-argument launch.
pub(super) fn play_args(shown: &Args, debug: bool) -> Args {
    let mut args = shown.clone();
    args.scaling = None;
    args.fullscreen = false;
    args.touch_ui = false;
    args.preset = None;
    args.game_profile = None;
    args.viewport = None;
    args.zoom = None;
    args.camera_follow = None;
    args.no_audio = false;
    args.preview.debug |= debug;
    args
}

/// The preset a boot on `args` and `config` would have, in the order a
/// start uses: the flag, then the manifests, then the settings. A manifest
/// that does not parse stops the boot, and says why then.
pub(super) fn next_preset(args: &Args, config: &config::Config) -> config::Preset {
    let modded = with_mod_settings(args, Some(config)).ok().flatten();
    args.preset
        .or(modded.as_ref().unwrap_or(config).preset)
        .unwrap_or_default()
}

/// The command line a boot starts from: a selector's toggle wins over
/// `--preset` and the manifests, for this boot.
pub(super) fn boot_args(pending: &Args, toggle: Option<config::Preset>) -> Args {
    let mut args = pending.clone();
    args.preset = toggle.or(args.preset);
    args
}

/// The launcher a start shows, if it shows one: with `--launcher`, or for
/// a start naming nothing whose settings are missing, malformed
/// (`problem`) or name game data that does not validate. The command
/// line's display flags pre-set its choices.
pub(super) fn launcher_for(
    args: &Args,
    config: Option<&config::Config>,
    problem: Option<String>,
) -> Option<launcher::Launcher> {
    let forced = args.launcher || problem.is_some();
    let launcher = launcher_on(args, config, problem);
    match launcher.ready() {
        // Data the launcher discovered is shown before it is played (and
        // saved by Play).
        Ok(()) if !forced && !launcher.discovered() => None,
        Ok(()) => Some(launcher),
        Err(reason) => {
            tracing::info!(%reason, "no valid game data set up: showing the launcher");
            Some(launcher)
        }
    }
}

/// The launcher on `config`, with the command line's display flags.
pub(super) fn launcher_on(
    args: &Args,
    config: Option<&config::Config>,
    problem: Option<String>,
) -> launcher::Launcher {
    let mut config = config.cloned().unwrap_or_default();
    if let Some(scaling) = args.scaling {
        config.scaling = Some(scaling);
    }
    if let Some(preset) = args.preset {
        config.preset = Some(preset);
    }
    if let Some(game) = args.game_profile {
        // That game's profile comes up into the launcher's fields.
        config.switch_game(game);
    }
    if args.fullscreen {
        config.fullscreen = Some(true);
    }
    if args.touch_ui {
        config.touch_ui = Some(true);
    }
    if let Some(viewport) = args.viewport {
        config.viewport_mode = Some(viewport);
    }
    if let Some(zoom) = args.zoom {
        config.default_zoom = Some(zoom);
    }
    if let Some(mode) = args.camera_follow {
        config.camera_follow = Some(mode);
    }
    if args.no_audio {
        config.audio = Some(false);
    }
    launcher::Launcher::new(launcher::Setup {
        config,
        settings_path: config::find(&config::search_dirs()),
        save_dirs: config::search_dirs(),
        start_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        problem,
        data_dirs: config::standard_data_dirs(),
    })
}

/// The settings file beside the executable or in the working directory.
/// Without one, when the launch wants game data the command line does not
/// name, each game's data discovered on disk, saved for the next launch:
/// the game the command line names is the active one, else the first
/// found.
fn settings(args: &Args) -> AppResult<Option<config::Config>> {
    if args.replay_inputs.is_some() {
        return Ok(None);
    }
    let dirs = config::search_dirs();
    if let Some(path) = config::find(&dirs) {
        tracing::info!(file = %path.display(), "settings loaded");
        return config::Config::load(&path).map(Some);
    }
    if !args.wants_data() {
        return Ok(None);
    }
    let profiles = config::discover_games(&config::standard_data_dirs());
    let Some(first) = config::GAMES
        .into_iter()
        .find(|game| !profiles.get(*game).is_empty())
    else {
        return Ok(None);
    };
    for game in config::GAMES {
        let paths = profiles.get(game);
        if !paths.is_empty() {
            tracing::info!(
                game = config::game_name(game),
                master = ?paths.master_dat,
                critter = ?paths.critter_dat,
                data = ?paths.data_root,
                "game data discovered"
            );
        }
    }
    let found = config::Config::with_profiles(args.game_profile.unwrap_or(first), profiles);
    config::persist(&found, &dirs);
    Ok(Some(found))
}

pub(crate) fn run(mut args: Args) -> AppResult<()> {
    if let (Some(output), Some(stage)) = (&args.capture_gallery, &args.capture_stage) {
        return gallery::capture(output, stage, args.render_path.unwrap_or_default());
    }
    if let Some(name) = &args.export_mod {
        // Only a settings file already there: an export never discovers
        // data or writes settings.
        let config = config::find(&config::search_dirs())
            .map(|path| config::Config::load(&path))
            .transpose()?;
        let request = mods::ExportRequest {
            name: name.clone(),
            output: args.export_output.clone(),
            from: args.export_from.clone(),
            viewport: args.viewport,
            zoom: args.zoom,
        };
        let (folder, files) = mods::export_setup(config.as_ref(), &request)?;
        println!(
            "Mod {name} exported to {} ({files} game file{}); add it in the launcher's Mods tab.",
            folder.display(),
            if files == 1 { "" } else { "s" }
        );
        return Ok(());
    }
    if let Some(folder) = &args.replay_bundle {
        let replayed = autoplay_runner::replay_bundle(folder)?;
        println!(
            "Bundle replayed: {} rows, state {}",
            replayed.rows,
            replayed.state_hash.as_deref().unwrap_or("none")
        );
        return Ok(());
    }
    if args.autoplay_headless {
        check_headless_start(&args)?;
        args.preview.game_profile = args.game_profile.unwrap_or_default();
        args.preview.mods = args.mod_list.clone().map(config::ModList::File);
        if args.profile.is_some() {
            let path = config::find(&config::search_dirs())
                .ok_or("--profile needs a settings file, and none was found")?;
            apply_headless_profile(&mut args, &config::Config::load(&path)?)?;
        }
        let mut options = autoplay_runner::AutoplayOptions::from_args(
            args.preview.clone(),
            args.autoplay_output.clone(),
        );
        options.ticks = args.ticks;
        options.chaos = args.chaos_doors_c4;
        let report = autoplay_runner::run_headless_autoplay(options)?;
        println!(
            "Autoplay: {:?} after {} ticks, {} actions, {} refusals, {} audits, {} chaos checks, {:.0} ticks/s; {}",
            report.ended,
            report.ticks,
            report.actions,
            report.refusals,
            report.tally.total(),
            report.bot.checks,
            report.ticks_per_second,
            report.output.display(),
        );
        return match report.failure {
            Some(failure) => Err(format!("autoplay failed: {:?}", failure.kind).into()),
            None => Ok(()),
        };
    }
    if let Some(path) = &args.replay_inputs {
        args.preview.headless = true;
        // A replay takes nothing from the settings, but the command line's
        // game profile chooses the data's formats (SPEC-015/016).
        args.preview.game_profile = args.game_profile.unwrap_or_default();
        args.preview.render_path = launch_render_path(&args, None);
        // SPEC-024a §2.3: the one way a replay mounts layers.
        args.preview.mods = args.mod_list.clone().map(config::ModList::File);
        // SPEC-021 §3.10: these fail with status 1, before the run starts.
        let expected = args
            .checkpoints_expect
            .as_deref()
            .map(|path| -> AppResult<_> {
                let text = std::fs::read_to_string(path)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                Ok(vault_core::checkpoints::ExpectedStream::parse(&text)
                    .map_err(|error| format!("{}: {error}", path.display()))?)
            })
            .transpose()?;
        if let Some(path) = &args.checkpoints_out {
            std::fs::File::create(path).map_err(|error| format!("{}: {error}", path.display()))?;
        }
        let session =
            session::SessionRuntime::start(session::SessionConfig::replay(args.preview.clone()))?
                .ok_or("--replay-inputs requires a preview asset")?;
        let log =
            std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let report = replay::replay_with_checkpoints(
            session,
            std::io::BufReader::new(log),
            args.replay_frame.as_deref(),
            replay::CheckpointOptions {
                out: args.checkpoints_out.clone(),
                expected,
            },
        )?;
        tracing::info!(
            commands = report.commands,
            elapsed_ms = report.elapsed_ms,
            "replay finished"
        );
        println!("{}", serde_json::to_string(&report)?);
        return Ok(());
    }
    // A start naming nothing may show the launcher instead, which also
    // reports a malformed settings file rather than failing on it.
    let mut bare = args.launcher || (!args.names_asset() && !args.smoke_test);
    let named_profile = args.profile.is_some();
    let (config, mut problem) = match settings(&args) {
        Ok(config) => (config, None),
        Err(error) if bare || named_profile => (None, Some(error.to_string())),
        Err(error) => return Err(error),
    };
    let mut config = config;
    // The desktop menu names a profile with no terminal to report to: one the
    // settings do not have is shown in the launcher.
    if let Some(name) = args.profile.take_if(|name| {
        !config.as_ref().is_some_and(|config| {
            config
                .profile_names()
                .iter()
                .any(|(known, _)| known == name)
        })
    }) {
        args.preview.game = false;
        bare = true;
        problem.get_or_insert(format!("No profile named {name} in the settings."));
    }
    let plan = boot_plan(
        &args,
        config.as_ref(),
        problem.is_none(),
        &config::standard_data_dirs(),
    );
    // SPEC-020 §2.4: one edition found boots at once, whichever the
    // settings last named. The choice is not recorded.
    if let Some((boot::Decision::Start(game), found)) = &plan
        && let Ok(paths) = found.get(*game)
    {
        let settings = config.get_or_insert_default();
        if settings.active_game() != *game || !settings.has_game_data() {
            settings.choose_game(*game, paths.clone());
        }
    }
    let screen = plan.as_ref().and_then(|(decision, _)| match decision {
        boot::Decision::Banner(game) => Some(boot::Screen::Banner(boot::Banner::new(*game))),
        boot::Decision::Selector => plan.as_ref().map(|(_, found)| {
            let none = config::Config::default();
            let settings = config.as_ref().unwrap_or(&none);
            boot::Screen::Selector(boot::Selector::new(settings, found.clone(), None))
        }),
        boot::Decision::Launcher | boot::Decision::Start(_) => None,
    });
    let launcher = (bare && screen.is_none())
        .then(|| launcher_for(&args, config.as_ref(), problem))
        .flatten();
    let input_log = InputLog::open(args.record_inputs.as_deref())?;
    let mut shell = Shell {
        smoke_test: args.smoke_test,
        input_log,
        autoplay_start: args
            .autoplay_watch
            .then(|| (args.preview.seed.unwrap_or(1), args.chaos_doors_c4)),
        switch_base: (!args.smoke_test).then(|| Box::new(switch_args(&args))),
        ..Default::default()
    };
    if let Some(screen) = screen {
        let shown = resolve_launch(&mut args.clone(), config.as_ref());
        shell.scaling = shown.scaling;
        shell.touch_ui = shown.touch_ui;
        shell.boot = Some(screen);
        shell.boot_config = config;
        shell.boot_found = plan.map(|(_, found)| found);
        shell.pending = Some(Box::new(args));
    } else if let Some(launcher) = launcher {
        let shown = resolve_launch(&mut args.clone(), config.as_ref());
        shell.scaling = shown.scaling;
        shell.touch_ui = shown.touch_ui;
        shell.launcher = Some(Box::new(launcher));
        shell.pending = Some(Box::new(args));
    } else {
        let (launch, session, audio) = start(
            &mut args,
            config.as_ref(),
            session::SessionOrigin::CommandLine,
        )?;
        shell.scaling = launch.scaling;
        shell.enhanced = launch.enhanced;
        shell.fullscreen = launch.fullscreen;
        shell.touch_ui = launch.touch_ui;
        shell.game_speed = launch_game_speed(&launch, config.as_ref());
        shell.toasts = if args.smoke_test {
            None
        } else {
            session_toasts(config.as_ref())
        };
        shell.hints_once = boot::hint_setting(config.as_ref()) == config::HotkeyHints::Once;
        shell._audio = audio;
        shell.display = session
            .as_ref()
            .map(|session| DisplayPalette::new(session.palette(), session.brightness()));
        shell.session = session;
    }
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let run = event_loop.run_app(&mut shell);
    if let Some(Err(error)) = shell.input_log.0.as_mut().map(std::io::Write::flush) {
        shell.input_log.stop(&error);
    }
    run?;
    if let Some(error) = shell.failure {
        return Err(error);
    }
    if shell.smoke_test && !shell.frame_presented {
        return Err("smoke test exited before presenting a frame".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "../warm_switch_tests.rs"]
mod warm_switch_tests;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
