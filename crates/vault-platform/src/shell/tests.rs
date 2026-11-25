use super::*;

fn parse(args: &[&str]) -> Args {
    Args::try_parse_from(std::iter::once("vault-platform").chain(args.iter().copied())).unwrap()
}

/// A writer the test reads back.
#[derive(Clone, Default)]
struct SharedLog(Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for SharedLog {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Watch Mode's frames with no window: its speed plays several logged
/// ticks a shown frame, the clock stays monotonic when the player takes
/// over, and the log replays to the same world with no agent running.
#[test]
fn watch_frames_log_a_session_a_replay_repeats() {
    use map_preview::bot::BotSpeed;
    use vault_fixtures::campaign::{self, Edition};
    let dir = map_preview::test_support::synthetic_corpus();
    campaign::write_campaign(dir.path(), Edition::Fallout1);
    let args = preview::PreviewArgs {
        corpus_dir: Some(dir.path().into()),
        preview_map: Some(campaign::start(Edition::Fallout1)),
        player_gcd: Some("premade/synthetic.gcd".into()),
        seed: Some(42),
        ..Default::default()
    };
    let start = || {
        session::SessionRuntime::start(session::SessionConfig::replay(args.clone()))
            .unwrap()
            .unwrap()
    };
    let log = SharedLog::default();
    let mut shell = Shell {
        session: Some(start()),
        input_log: InputLog(Some(Box::new(log.clone()))),
        autoplay_start: Some((42, true)),
        ..Default::default()
    };
    // The first record of every session the shell logs.
    let [width, height] = replay::INITIAL_SURFACE;
    shell.record(InputCommand::Surface { width, height });
    shell.start_watch();
    assert!(shell.bot_has_control());
    let world = |shell: &mut Shell| {
        let session = shell.session.as_mut().unwrap();
        replay::interactive_world(session)
            .unwrap()
            .host
            .world
            .state_hash()
    };
    let watch = |shell: &mut Shell| -> Option<Box<map_preview::bot::Watch>> {
        let session = shell.session.as_mut().unwrap();
        replay::interactive_world(session).unwrap().watch.take()
    };
    let mut held = watch(&mut shell).unwrap();
    held.speed = BotSpeed::X4;
    replay::interactive_world(shell.session.as_mut().unwrap())
        .unwrap()
        .watch = Some(held);
    let frames = 400;
    for frame in 0..frames {
        let now = (frame + 1) * WATCH_FRAME_MS + shell.watch_ahead;
        shell.watch_frames(now).unwrap();
        shell.log_autoplay_change();
    }
    assert_eq!(shell.watch_ahead, frames * 3 * WATCH_FRAME_MS);
    let text = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    let ticks = text
        .lines()
        .filter(|line| line.contains(r#""kind":"tick""#));
    assert_eq!(ticks.count() as u64, frames * 4, "four ticks a shown frame");
    assert!(text.contains(r#""kind":"bot""#), "the bot played");
    // The takeover: the bot waits, and the session runs on the plain path.
    assert!(watch(&mut shell).is_some());
    assert!(!shell.bot_has_control(), "the bot is out of the session");

    let expected = world(&mut shell);
    let mut replayed = replay::Replay::new(start()).unwrap();
    replayed.run(std::io::Cursor::new(text)).unwrap();
    let hash = replayed.world().unwrap().host.world.state_hash();
    assert_eq!(hash, expected, "the replay reached the same world");
}

/// A desktop action's flags: the game starts, under that profile, and a
/// named profile is no zero-argument start.
#[test]
fn the_profile_flag_chooses_the_profile_for_one_start() {
    let args = parse(&["--profile", "ettu", "--game"]);
    assert_eq!(args.profile.as_deref(), Some("ettu"));
    assert!(args.preview.game);
    assert!(!parse(&["--profile", "ettu"]).zero_argument());
    assert!(parse(&[]).zero_argument());
    assert!(
        Args::try_parse_from([
            "vault-platform",
            "--profile",
            "ettu",
            "--game-profile",
            "fallout1"
        ])
        .is_err()
    );
}

/// `--profile` gives a headless run that profile's edition, data, links,
/// mods and sfall, with its rules left Vanilla, and refuses a name the
/// settings do not have rather than running on nothing.
#[test]
fn a_headless_run_takes_its_session_from_the_profile() {
    let settings = config::Config::parse(concat!(
        "master_dat = \"/games/f1/MASTER.DAT\"\n",
        "critter_dat = \"/games/f1/CRITTER.DAT\"\n",
        "[profiles.fallout2]\n",
        "master_dat = \"/games/ettu/MASTER.DAT\"\n",
        "critter_dat = \"/games/ettu/CRITTER.DAT\"\n",
        "[profiles.ettu]\n",
        "edition = \"fallout2\"\n",
        "secondary_dat_dirs = [\"/games/f1\"]\n",
        "sfall = true\n",
        "preset = \"enhanced\"\n",
    ))
    .unwrap();
    let start = ["--autoplay-headless", "--preview-map", "maps/start.map"];
    let mut args = parse(&[&start[..], &["--profile", "ettu"]].concat());
    apply_headless_profile(&mut args, &settings).unwrap();
    let session = &args.preview;
    assert_eq!(session.game_profile, vault_core::GameProfile::Fallout2);
    assert_eq!(
        session.master_dat.as_deref(),
        Some(Path::new("/games/ettu/MASTER.DAT"))
    );
    assert_eq!(session.linked_dirs, [PathBuf::from("/games/f1")]);
    assert!(session.sfall);
    assert_eq!(session.preset, vault_core::EnginePreset::Vanilla);

    // The command line still wins over the profile's data.
    let mut args = parse(&[&start[..], &["--profile", "ettu", "--corpus-dir", "/here"]].concat());
    apply_headless_profile(&mut args, &settings).unwrap();
    assert_eq!(args.preview.corpus_dir.as_deref(), Some(Path::new("/here")));
    assert_eq!(args.preview.master_dat, None);

    let mut args = parse(&[&start[..], &["--profile", "nobody"]].concat());
    let error = apply_headless_profile(&mut args, &settings).unwrap_err();
    assert_eq!(error.to_string(), "no profile named nobody in the settings");
}

/// `--game` passes the command line's check, but a run needs a map.
#[test]
fn a_headless_run_needs_a_map() {
    let error = check_headless_start(&parse(&["--autoplay-headless", "--game"])).unwrap_err();
    assert!(error.to_string().contains("--preview-map"), "{error}");
    let named = parse(&["--autoplay-headless", "--preview-map", "maps/a.map"]);
    assert!(check_headless_start(&named).is_ok());
}

#[test]
fn the_sfall_flag_parses_and_defaults_off() {
    assert!(parse(&["--sfall"]).preview.sfall);
    let list = parse(&["--mod-list", "order.toml"]).mod_list;
    assert_eq!(list.as_deref(), Some(Path::new("order.toml")));
    assert!(!parse(&[]).preview.sfall);
}

fn installed() -> config::Config {
    config::Config {
        master_dat: Some("/games/fallout/MASTER.DAT".into()),
        critter_dat: Some("/games/fallout/CRITTER.DAT".into()),
        data_root: Some("/games/fallout/DATA".into()),
        scaling: Some(ScalingMode::AspectFit),
        fullscreen: Some(true),
        debug: Some(true),
        preset: Some(config::Preset::Enhanced),
        ..Default::default()
    }
}

#[test]
fn no_arguments_start_the_game_from_the_settings() {
    let mut args = parse(&[]);
    assert!(args.wants_data());
    let launch = resolve_launch(&mut args, Some(&installed()));
    assert!(launch.zero_arg_game);
    assert!(args.preview.game && args.preview.debug);
    assert_eq!(
        args.preview.master_dat,
        Some("/games/fallout/MASTER.DAT".into())
    );
    assert_eq!(
        args.preview.critter_dat,
        Some("/games/fallout/CRITTER.DAT".into())
    );
    assert_eq!(args.preview.corpus_dir, Some("/games/fallout/DATA".into()));
    assert_eq!(
        launch,
        Launch {
            scaling: ScalingMode::AspectFit,
            fullscreen: true,
            touch_ui: false,
            game: vault_core::GameProfile::Fallout1,
            preset: config::Preset::Enhanced,
            enhanced: Some(enhanced::EnhancedSettings::default()),
            mod_list: None,
            zero_arg_game: true,
            audio: true,
        }
    );
    // Without settings, or with settings naming no data, the color test
    // shows as before.
    let mut args = parse(&[]);
    let launch = resolve_launch(&mut args, None);
    assert!(!launch.zero_arg_game && !args.names_asset());
    assert_eq!(
        (launch.scaling, launch.fullscreen, launch.preset),
        (ScalingMode::AspectFit, false, config::Preset::Vanilla)
    );
    let display_only = config::Config {
        fullscreen: Some(true),
        master_dat: Some("/only/MASTER.DAT".into()),
        ..Default::default()
    };
    let mut args = parse(&[]);
    let launch = resolve_launch(&mut args, Some(&display_only));
    assert!(!launch.zero_arg_game && !args.names_asset() && launch.fullscreen);
    // A loose data directory alone is game data.
    let loose = config::Config {
        data_root: Some("/unpacked".into()),
        ..Default::default()
    };
    let mut args = parse(&[]);
    assert!(resolve_launch(&mut args, Some(&loose)).zero_arg_game);
    assert_eq!(
        (args.preview.corpus_dir, args.preview.master_dat),
        (Some("/unpacked".into()), None)
    );
}

#[test]
fn command_line_flags_win_over_the_settings() {
    let mut args = parse(&[
        "--game",
        "--master-dat",
        "/cli/master.dat",
        "--critter-dat",
        "/cli/critter.dat",
        "--scaling",
        "integer",
        "--preset",
        "vanilla",
    ]);
    assert!(!args.wants_data());
    let launch = resolve_launch(&mut args, Some(&installed()));
    assert!(!launch.zero_arg_game);
    // The command line's source is used whole: no settings data mixes in.
    assert_eq!(args.preview.master_dat, Some("/cli/master.dat".into()));
    assert_eq!(args.preview.critter_dat, Some("/cli/critter.dat".into()));
    assert_eq!(args.preview.corpus_dir, None);
    assert_eq!(launch.scaling, ScalingMode::Integer);
    assert_eq!(launch.preset, config::Preset::Vanilla);
    // Unset flags take the settings.
    assert!(launch.fullscreen && args.preview.debug);
    // `--game` without a source takes the settings' data.
    let mut args = parse(&["--game", "--scaling", "aspect_fit"]);
    let launch = resolve_launch(&mut args, Some(&installed()));
    assert!(!launch.zero_arg_game);
    assert_eq!(
        args.preview.master_dat,
        Some("/games/fallout/MASTER.DAT".into())
    );
    assert_eq!(launch.scaling, ScalingMode::AspectFit);
    // A map preview with its own corpus keeps it.
    let mut args = parse(&["--corpus-dir", "/cli", "--preview-map", "m.map"]);
    resolve_launch(&mut args, Some(&installed()));
    assert!(!args.preview.game);
    assert_eq!(
        (args.preview.corpus_dir, args.preview.master_dat),
        (Some("/cli".into()), None)
    );
    // A bare smoke test stays the color test.
    let mut args = parse(&["--smoke-test"]);
    assert!(!args.wants_data());
    assert!(!resolve_launch(&mut args, Some(&installed())).zero_arg_game);
    assert!(!args.preview.game);
    // A replay takes nothing from the settings.
    let mut args = parse(&["--replay-inputs", "log.jsonl", "--preview-map", "m.map"]);
    let launch = resolve_launch(&mut args, Some(&installed()));
    assert!(!args.preview.debug && args.preview.master_dat.is_none());
    // (Its own pointer mapping follows the log's `scaling` records.)
    assert_eq!(
        (launch.scaling, launch.fullscreen),
        (ScalingMode::AspectFit, false)
    );
    // The value flags check their values.
    for bad in [["--scaling", "stretch"], ["--preset", "modern"]] {
        assert!(Args::try_parse_from(["vault-platform", bad[0], bad[1]]).is_err());
    }
    assert!(
        Args::try_parse_from([
            "vault-platform",
            "--record-inputs",
            "-",
            "--replay-inputs",
            "log.jsonl",
            "--preview-map",
            "m.map"
        ])
        .is_err()
    );
}

#[test]
fn the_launchers_play_builds_the_zero_argument_launch() {
    let shown = parse(&["--launcher", "--scaling", "integer", "--no-audio"]);
    assert!(shown.launcher && shown.wants_data() && !shown.names_asset());
    // What the launcher saved and hands over (its fields made absolute).
    let chosen = config::Config {
        scaling: Some(ScalingMode::AspectFit),
        audio: Some(false),
        debug: None,
        mod_list: Some(config::ModList::File(
            "/games/fallout/load_order.toml".into(),
        )),
        ..installed()
    };
    let mut args = play_args(&shown, false);
    let launch = resolve_launch(&mut args, Some(&chosen));
    assert!(args.preview.game && launch.zero_arg_game && !args.preview.debug);
    assert_eq!(
        (
            args.preview.master_dat,
            args.preview.critter_dat,
            args.preview.corpus_dir
        ),
        (
            Some("/games/fallout/MASTER.DAT".into()),
            Some("/games/fallout/CRITTER.DAT".into()),
            Some("/games/fallout/DATA".into())
        )
    );
    // The launcher's choices, not the flags it was shown with.
    assert_eq!(
        launch,
        Launch {
            scaling: ScalingMode::AspectFit,
            fullscreen: true,
            touch_ui: false,
            game: vault_core::GameProfile::Fallout1,
            preset: config::Preset::Enhanced,
            enhanced: Some(enhanced::EnhancedSettings::default()),
            mod_list: chosen.mod_list.clone(),
            zero_arg_game: true,
            audio: false,
        }
    );
    // Launch with Debug, and audio back on.
    let mut args = play_args(&shown, true);
    let on = config::Config {
        audio: Some(true),
        ..chosen
    };
    let launch = resolve_launch(&mut args, Some(&on));
    assert!(args.preview.debug && launch.audio);
    // `--no-audio` still wins over the settings without the launcher.
    let mut args = parse(&["--no-audio"]);
    assert!(!resolve_launch(&mut args, Some(&on)).audio);
    // The launcher is never shown for a replay, a smoke test or an asset.
    for other in [
        &["--smoke-test"][..],
        &["--game"],
        &["--preview-map", "m.map"],
        &["--replay-inputs", "log.jsonl", "--preview-map", "m.map"],
    ] {
        let line = std::iter::once("vault-platform")
            .chain(["--launcher"])
            .chain(other.iter().copied());
        assert!(Args::try_parse_from(line).is_err(), "{other:?}");
    }
}

#[test]
fn the_input_log_is_written_only_when_asked_for() {
    use std::io::Write;
    assert!(InputLog::open(None).unwrap().0.is_none());
    assert!(parse(&[]).record_inputs.is_none());
    assert_eq!(
        parse(&["--record-inputs", "-"]).record_inputs,
        Some(PathBuf::from("-"))
    );
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inputs.jsonl");
    let mut log = InputLog::open(Some(&path)).unwrap();
    let mut ids = SequenceIds::default();
    input::write_command(log.0.as_mut().unwrap(), &mut ids, &InputCommand::Quit).unwrap();
    log.0.as_mut().unwrap().flush().unwrap();
    drop(log);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "{\"command_id\":1,\"kind\":\"quit\"}\n"
    );
    assert!(InputLog::open(Some(&temp.path().join("missing/inputs.jsonl"))).is_err());
}

#[test]
fn the_window_opens_at_scale_two_or_the_largest_fitting_multiple() {
    assert_eq!(initial_window_size(None), [1280, 960]);
    assert_eq!(initial_window_size(Some([0, 0])), [1280, 960]);
    assert_eq!(initial_window_size(Some([3840, 2160])), [1280, 960]);
    assert_eq!(initial_window_size(Some([1280, 960])), [1280, 960]);
    assert_eq!(initial_window_size(Some([1366, 768])), [640, 480]);
    assert_eq!(initial_window_size(Some([1279, 1024])), [640, 480]);
    assert_eq!(initial_window_size(Some([800, 400])), [640, 480]);
}

#[test]
fn alt_f_cycles_the_game_speed_under_the_enhanced_preset_only() {
    use enhanced::GameSpeed;
    assert_eq!(
        qol_hotkey(KeyCode::KeyF, true, true),
        Some(ShellHotkey::GameSpeed)
    );
    // Plain F, and Alt+F under Vanilla, stay the game's.
    assert_eq!(qol_hotkey(KeyCode::KeyF, false, true), None);
    assert_eq!(qol_hotkey(KeyCode::KeyF, true, false), None);
    // Alt+H is the world's own (the overlay), never the shell's.
    assert_eq!(qol_hotkey(KeyCode::KeyH, true, true), None);
    assert_eq!(shell_hotkey(KeyCode::KeyH, true, true), None);
    // The cycle: 1.0, 1.5, 2.0, then 1.0 again.
    let mut speed = GameSpeed::Normal;
    let mut seen = Vec::new();
    for _ in 0..3 {
        speed = speed.next();
        seen.push(speed.label());
    }
    assert_eq!(seen, ["1.5x", "2.0x", "1.0x"]);
}

#[test]
fn the_settings_game_speed_runs_under_the_enhanced_preset_only() {
    use enhanced::GameSpeed;
    let settings = config::Config {
        game_speed: Some(GameSpeed::Faster),
        ..installed()
    };
    let mut args = parse(&["--preset", "enhanced"]);
    let launch = resolve_launch(&mut args, Some(&settings));
    assert_eq!(
        launch_game_speed(&launch, Some(&settings)),
        Some(GameSpeed::Faster)
    );
    // Without the key, 1.0x.
    assert_eq!(
        launch_game_speed(&launch, Some(&installed())),
        Some(GameSpeed::Normal)
    );
    // Vanilla has none (Alt+F is off and nothing is logged).
    let mut args = parse(&["--preset", "vanilla"]);
    let launch = resolve_launch(&mut args, Some(&settings));
    assert_eq!(launch_game_speed(&launch, Some(&settings)), None);
}

#[test]
fn the_shell_hotkeys_leave_f10_to_the_game() {
    assert_eq!(
        shell_hotkey(KeyCode::F11, false, false),
        Some(ShellHotkey::Fullscreen)
    );
    assert_eq!(
        shell_hotkey(KeyCode::Enter, true, false),
        Some(ShellHotkey::Fullscreen)
    );
    assert_eq!(
        shell_hotkey(KeyCode::NumpadEnter, true, false),
        Some(ShellHotkey::Fullscreen)
    );
    assert_eq!(
        shell_hotkey(KeyCode::KeyS, true, false),
        Some(ShellHotkey::Scaling)
    );
    // SPEC-014: F10 is the game's quit confirmation; plain Enter and S
    // are the game's keys.
    for alt in [false, true] {
        assert_eq!(shell_hotkey(KeyCode::F10, alt, false), None);
    }
    assert_eq!(shell_hotkey(KeyCode::Enter, false, false), None);
    assert_eq!(shell_hotkey(KeyCode::KeyS, false, false), None);
    // Alt+V is the shell's in an enhanced session only; V and Alt+V
    // otherwise reach the game as before.
    assert_eq!(
        shell_hotkey(KeyCode::KeyV, true, true),
        Some(ShellHotkey::Viewport)
    );
    assert_eq!(shell_hotkey(KeyCode::KeyV, true, false), None);
    assert_eq!(shell_hotkey(KeyCode::KeyV, false, true), None);
    assert_eq!(
        shell_hotkey(KeyCode::KeyS, true, true),
        Some(ShellHotkey::Scaling)
    );
}

#[test]
fn a_fallout2_launch_resolves_its_profile() {
    let settings = config::Config {
        game: Some(vault_core::GameProfile::Fallout2),
        ..installed()
    };
    assert_eq!(
        resolve_launch(&mut parse(&[]), Some(&settings)).game,
        vault_core::GameProfile::Fallout2
    );
    // The flag wins over the file, as the other flags do.
    let mut args = parse(&["--game-profile", "fallout1"]);
    assert_eq!(
        resolve_launch(&mut args, Some(&settings)).game,
        vault_core::GameProfile::Fallout1
    );
    assert!(Args::try_parse_from(["vault-platform", "--game-profile", "fallout3"]).is_err());
    // The flag's game launches with that game's own profile; a file
    // without one (from before profiles) gives its top-level paths.
    assert_eq!(
        args.preview.master_dat,
        Some("/games/fallout/MASTER.DAT".into())
    );
    let mut profiled = settings.clone();
    profiled.profiles.fallout1 = config::GamePaths {
        master_dat: Some("/fo1/MASTER.DAT".into()),
        critter_dat: Some("/fo1/CRITTER.DAT".into()),
        data_root: None,
    };
    let mut args = parse(&["--game-profile", "fallout1"]);
    resolve_launch(&mut args, Some(&profiled));
    assert_eq!(args.preview.master_dat, Some("/fo1/MASTER.DAT".into()));
    assert_eq!(args.preview.corpus_dir, None);
    // SPEC-018 §6.7: Play is not refused for being Fallout 2; without
    // game data it fails on its sources, as Fallout 1 does.
    let preview = preview::PreviewArgs {
        game: true,
        game_profile: vault_core::GameProfile::Fallout2,
        ..Default::default()
    };
    let error = preview.load().err().unwrap();
    assert!(error.to_string().contains("--master-dat"), "{error}");
    assert_eq!(
        play_args(&parse(&["--game-profile", "fallout2"]), false).game_profile,
        None
    );
}

#[test]
fn mod_manifests_set_the_launch_under_the_command_line() {
    let dir = tempfile::tempdir().unwrap();
    let layer = dir.path().join("mods/camera");
    std::fs::create_dir_all(&layer).unwrap();
    std::fs::write(
        layer.join("vault-mod.toml"),
        "name = \"Camera\"\n[enhanced]\npreset = \"enhanced\"\ndefault_zoom = 2.0\n\
         roof_peek = \"cursor_xray\"\nsmooth_camera = false",
    )
    .unwrap();
    let load_order = dir.path().join("load_order.toml");
    std::fs::write(&load_order, "layers = [\"mods/camera\"]").unwrap();
    let settings = config::Config {
        preset: Some(config::Preset::Vanilla),
        roof_peek: Some(enhanced::RoofPeek::Hidden),
        mod_list: Some(config::ModList::File(load_order.clone())),
        ..installed()
    };
    let launch = |flags: &[&str]| {
        let mut args = parse(flags);
        let modded = with_mod_settings(&args, Some(&settings)).unwrap();
        resolve_launch(&mut args, modded.as_ref().or(Some(&settings)))
    };
    // The mod's settings win over the file's, preset included.
    let launch_plain = launch(&[]);
    assert_eq!(launch_plain.preset, config::Preset::Enhanced);
    assert_eq!(
        launch_plain.enhanced,
        Some(enhanced::EnhancedSettings {
            default_zoom: 2.0,
            smooth_camera: false,
            roof_peek: enhanced::RoofPeek::CursorXray,
            ..Default::default()
        })
    );
    // The command line's flags win over the mod's.
    let flagged = launch(&["--zoom", "1.5"]);
    assert_eq!(
        flagged.enhanced.map(|settings| settings.default_zoom),
        Some(1.5)
    );
    assert_eq!(launch(&["--preset", "vanilla"]).enhanced, None);
    // A replay takes nothing from the settings or the mods.
    let replay = parse(&["--replay-inputs", "log.jsonl", "--game"]);
    assert!(
        with_mod_settings(&replay, Some(&settings))
            .unwrap()
            .is_none()
    );
    // A malformed manifest stops the launch.
    std::fs::write(layer.join("vault-mod.toml"), "name = \"Camera\"\nzoom = 2").unwrap();
    let error = with_mod_settings(&parse(&[]), Some(&settings)).unwrap_err();
    assert!(error.to_string().starts_with("mod_list:"), "{error}");
}

#[test]
fn the_enhanced_settings_reach_the_launch_with_the_enhanced_preset_only() {
    let settings = config::Config {
        viewport_mode: Some(enhanced::ViewportMode::Fixed),
        default_zoom: Some(1.5),
        roof_peek: Some(enhanced::RoofPeek::Hidden),
        ..installed()
    };
    let mut args = parse(&[]);
    let launch = resolve_launch(&mut args, Some(&settings));
    let expected = enhanced::EnhancedSettings {
        viewport: enhanced::ViewportMode::Fixed,
        default_zoom: 1.5,
        roof_peek: enhanced::RoofPeek::Hidden,
        ..Default::default()
    };
    assert_eq!(launch.enhanced, Some(expected));
    // The flags win over the file.
    let mut args = parse(&["--viewport", "unlocked_widescreen", "--zoom", "2.5"]);
    let launch = resolve_launch(&mut args, Some(&settings));
    assert_eq!(
        launch.enhanced,
        Some(enhanced::EnhancedSettings {
            viewport: enhanced::ViewportMode::Unlocked,
            default_zoom: 2.5,
            ..expected
        })
    );
    // Vanilla carries none, whatever the file and flags say.
    let mut args = parse(&["--preset", "vanilla", "--zoom", "2"]);
    assert_eq!(resolve_launch(&mut args, Some(&settings)).enhanced, None);
    assert_eq!(resolve_launch(&mut parse(&[]), None).enhanced, None);
    // The values are checked, and Play drops the flags as it does the
    // other display flags.
    for bad in [["--viewport", "wide"], ["--zoom", "4"], ["--zoom", "x"]] {
        assert!(Args::try_parse_from(["vault-platform", bad[0], bad[1]]).is_err());
    }
    let shown = parse(&["--launcher", "--viewport", "fixed_640x480", "--zoom", "3"]);
    let args = play_args(&shown, false);
    assert_eq!((args.viewport, args.zoom), (None, None));
}

/// SPEC-022 §3.1: `--render-path` wins over the settings' `render_path`,
/// which wins over the reference default; a replay takes the flag only;
/// Play keeps the flag (the launcher never saves it); anything else is
/// refused. The flag changes nothing else about the launch.
#[test]
fn the_render_path_flag_wins_over_the_settings_and_defaults_to_reference() {
    use vault_render::RenderPath::{Instanced, Reference};
    let file = config::Config {
        render_path: Some(Instanced),
        ..installed()
    };
    assert_eq!(launch_render_path(&parse(&[]), None), Reference);
    assert_eq!(
        launch_render_path(&parse(&[]), Some(&installed())),
        Reference
    );
    assert_eq!(launch_render_path(&parse(&[]), Some(&file)), Instanced);
    let flagged = parse(&["--render-path", "reference"]);
    assert_eq!(launch_render_path(&flagged, Some(&file)), Reference);
    let flagged = parse(&["--render-path", "instanced"]);
    assert_eq!(flagged.render_path, Some(Instanced));
    assert_eq!(launch_render_path(&flagged, None), Instanced);
    // A replay reads no settings.
    let replay = parse(&["--replay-inputs", "log.jsonl", "--preview-map", "m.map"]);
    assert_eq!(launch_render_path(&replay, Some(&file)), Reference);
    let replay = parse(&[
        "--replay-inputs",
        "log.jsonl",
        "--preview-map",
        "m.map",
        "--render-path",
        "instanced",
    ]);
    assert_eq!(launch_render_path(&replay, Some(&file)), Instanced);
    // The rest of the launch is the same with or without it.
    let plain = resolve_launch(&mut parse(&[]), Some(&installed()));
    let instanced = resolve_launch(&mut parse(&["--render-path", "instanced"]), Some(&file));
    assert_eq!(plain, instanced);
    // Play keeps the flag; bad values are refused.
    let shown = parse(&["--launcher", "--render-path", "instanced"]);
    assert_eq!(play_args(&shown, false).render_path, Some(Instanced));
    for bad in ["gpu", "Instanced", ""] {
        assert!(Args::try_parse_from(["vault-platform", "--render-path", bad]).is_err());
    }
}

#[test]
fn the_touch_ui_merges_like_fullscreen_and_play_drops_its_flag() {
    let on = config::Config {
        touch_ui: Some(true),
        ..installed()
    };
    let off = config::Config {
        touch_ui: Some(false),
        ..installed()
    };
    // Off by default; the file turns it on; the flag wins over `false`.
    assert!(!resolve_launch(&mut parse(&[]), Some(&installed())).touch_ui);
    assert!(!resolve_launch(&mut parse(&[]), None).touch_ui);
    assert!(resolve_launch(&mut parse(&[]), Some(&on)).touch_ui);
    assert!(resolve_launch(&mut parse(&["--touch-ui"]), Some(&off)).touch_ui);
    assert!(resolve_launch(&mut parse(&["--touch-ui"]), None).touch_ui);
    // A replay takes nothing from the settings: its log says.
    let mut replay = parse(&["--replay-inputs", "log.jsonl", "--preview-map", "m.map"]);
    assert!(!resolve_launch(&mut replay, Some(&on)).touch_ui);
    // Play drops the flag (the launcher showed it as its choice).
    let shown = parse(&["--launcher", "--touch-ui"]);
    assert!(shown.touch_ui);
    let mut args = play_args(&shown, false);
    assert!(!args.touch_ui);
    assert!(!resolve_launch(&mut args, Some(&off)).touch_ui);
    // Under Vanilla the touch UI brings the navigation-only unlocked
    // view (pan and zoom, the Vanilla interface otherwise); without it
    // Vanilla has no enhanced view, and the enhanced preset keeps its
    // full one.
    let vanilla = |touch: bool| config::Config {
        preset: Some(config::Preset::Vanilla),
        viewport_mode: Some(enhanced::ViewportMode::Fixed),
        roof_peek: Some(enhanced::RoofPeek::Hidden),
        default_zoom: Some(1.5),
        touch_ui: Some(touch),
        ..installed()
    };
    let navigation = resolve_launch(&mut parse(&[]), Some(&vanilla(true)));
    assert_eq!(navigation.preset, config::Preset::Vanilla);
    assert_eq!(
        navigation.enhanced,
        Some(enhanced::EnhancedSettings {
            viewport: enhanced::ViewportMode::Unlocked,
            default_zoom: 1.5,
            roof_peek: enhanced::RoofPeek::Vanilla,
            navigation_only: true,
            ..enhanced::EnhancedSettings::default()
        })
    );
    assert_eq!(
        resolve_launch(&mut parse(&[]), Some(&vanilla(false))).enhanced,
        None
    );
    let full = resolve_launch(&mut parse(&[]), Some(&on)).enhanced.unwrap();
    assert!(!full.navigation_only);
}

fn image(width: u32, height: u32, pixels: impl IntoIterator<Item = u8>) -> IndexedImage {
    IndexedImage::from_pixels(width, height, pixels.into_iter().collect()).unwrap()
}

#[test]
fn side_panels_tile_from_the_bar_outward() {
    // 1000 x 480 at interface scale 1: the 640-pixel frame at x 180,
    // so each side is 180 x 100 interface pixels.
    let layout = EnhancedLayout::compute([1000, 480], Some(1.0), 1.0).unwrap();
    let panel = image(4, 2, [1, 2, 3, 4, 5, 6, 7, 8]);
    let tiles = side_tiles(&layout, &panel);
    assert_eq!(tiles.len(), 2);
    let (left, left_rect) = &tiles[0];
    let (right, right_rect) = &tiles[1];
    assert_eq!((left.width(), left.height()), (180, 100));
    assert_eq!(*left_rect, [0.0, 380.0, 180.0, 100.0]);
    assert_eq!(*right_rect, [820.0, 380.0, 180.0, 100.0]);
    // The left tile ends on the panel's last column next to the bar,
    // and the right one starts on its first; rows repeat.
    assert_eq!(&left.pixels()[176..180], &[1, 2, 3, 4]);
    assert_eq!(&right.pixels()[..4], &[1, 2, 3, 4]);
    assert_eq!(&right.pixels()[180..184], &[5, 6, 7, 8]);
    assert_eq!(&right.pixels()[360..364], &[1, 2, 3, 4]);
    // At a fractional scale the tile rounds up and overhangs outward.
    let layout = EnhancedLayout::compute([1001, 720], Some(1.5), 1.0).unwrap();
    let tiles = side_tiles(&layout, &panel);
    let [x, _, w, h] = layout.side_rects[0];
    let (left, rect) = &tiles[0];
    assert_eq!(left.width(), (w / 1.5).ceil() as u32);
    assert_eq!(rect[0] + rect[2], x + w);
    assert_eq!(rect[3], h);
    // A frame spanning the window has no side panels.
    let layout = EnhancedLayout::compute([640, 480], None, 1.0).unwrap();
    assert!(side_tiles(&layout, &panel).is_empty());
}

#[test]
fn the_enhanced_layers_go_world_sides_interface_cursor() {
    let layout = EnhancedLayout::compute([1280, 480], Some(1.0), 2.0).unwrap();
    let frame = enhanced::EnhancedFrame {
        world: IndexedImage::new(layout.world_size[0], layout.world_size[1]).unwrap(),
        ui: IndexedImage::new(640, 480).unwrap(),
        ui_transparent: Some(0),
        side_panel: Some(image(2, 1, [9, 9])),
        bezel: false,
        cursor: Some((image(3, 2, [0; 6]), [1, 2])),
    };
    let sides = side_tiles(&layout, frame.side_panel.as_ref().unwrap());
    let shown = EnhancedShown {
        layout,
        frame,
        sides,
        bezel: None,
    };
    let layers = enhanced_layers(&shown, Some([700.0, 300.0]));
    let rects: Vec<[f32; 4]> = layers.iter().map(|layer| layer.rect).collect();
    assert_eq!(
        rects,
        [
            layout.world_layer_rect(),
            [0.0, 380.0, 320.0, 100.0],
            [960.0, 380.0, 320.0, 100.0],
            layout.ui_rect,
            [699.0, 298.0, 3.0, 2.0],
        ]
    );
    let transparent: Vec<Option<u8>> = layers.iter().map(|layer| layer.transparent).collect();
    assert_eq!(transparent, [None, None, None, Some(0), Some(0)]);
    assert!(layers.iter().all(|layer| layer.sharp));
    // Without a pointer there is no cursor layer.
    assert_eq!(enhanced_layers(&shown, None).len(), 4);
    // At interface scale 2 the cursor's hotspot and size double.
    let layout = EnhancedLayout::compute([1280, 960], Some(2.0), 1.0).unwrap();
    let shown = EnhancedShown { layout, ..shown };
    let layers = enhanced_layers(&shown, Some([700.0, 300.0]));
    assert_eq!(layers.last().unwrap().rect, [698.0, 296.0, 6.0, 4.0]);
}

#[test]
fn a_full_screen_window_draws_the_bezel_in_place_of_the_world() {
    // 1920x1080 at the auto interface scale (2.25): the frame spans
    // the height, 240 surface pixels each side.
    let layout = EnhancedLayout::compute([1920, 1080], None, 1.0).unwrap();
    let mut palette = [[128, 128, 128, 255]; 256];
    for (index, entry) in palette.iter_mut().enumerate() {
        let value = index as u8;
        *entry = [value, value, value / 2, 255];
    }
    let (bezel, rect) = bezel_layer(layout.surface, layout.ui_rect, &palette).unwrap();
    // The same layout and palette draw the same bezel.
    let (again, _) = bezel_layer(layout.surface, layout.ui_rect, &palette).unwrap();
    assert_eq!(bezel.pixels(), again.pixels());
    // It covers the surface, the frame's corner on a bezel pixel's.
    assert!(rect[0] <= 0.0 && rect[0] + rect[2] >= 1920.0);
    assert!(rect[1] <= 0.0 && rect[1] + rect[3] >= 1080.0);
    let per = rect[2] / bezel.width() as f32;
    assert!((per - layout.ui_scale).abs() < 1e-4);
    let hole = (layout.ui_rect[0] - rect[0]) / per;
    assert!((hole - hole.round()).abs() < 1e-3);
    // Never the cycling range or index 0.
    assert!(bezel.pixels().iter().all(|index| (1..229).contains(index)));
    let frame = enhanced::EnhancedFrame {
        world: IndexedImage::new(layout.world_size[0], layout.world_size[1]).unwrap(),
        ui: IndexedImage::new(640, 480).unwrap(),
        ui_transparent: None,
        side_panel: None,
        bezel: true,
        cursor: None,
    };
    let shown = EnhancedShown {
        layout,
        frame,
        sides: Vec::new(),
        bezel: Some((bezel, rect)),
    };
    let layers = enhanced_layers(&shown, None);
    let rects: Vec<[f32; 4]> = layers.iter().map(|layer| layer.rect).collect();
    assert_eq!(rects, [rect, layout.ui_rect]);
    // Composed, the frame (index 0 here) shows over it, and the bezel
    // all around it.
    let surface = vault_render::compose_layers(&layers, [1920, 1080], 0).unwrap();
    for (index, pixel) in surface.pixels().iter().enumerate() {
        let x = index % 1920;
        assert_eq!(*pixel == 0, (240..1680).contains(&x), "{x}");
    }
}
