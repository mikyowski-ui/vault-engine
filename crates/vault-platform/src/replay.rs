//! Headless, deterministic replay of a captured input log (`--replay-inputs`):
//! the JSONL commands the desktop writes to stdout are applied to the same
//! preview the desktop would show, with no window, GPU, or audio device, and
//! the final state is summarized as one SHA-256 digest for regression tests.
//!
//! The desktop logs its window's initial inner size (`surface`, the first
//! record) and every frame's clock (`tick`), so the replay starts from the
//! same picking geometry and ticks the preview at exactly the logged times,
//! between the same commands, with the same engine key repeats (SPEC-014
//! §3.29). A log without a leading `surface` record comes from an older
//! desktop: its time is simulated, one 16 ms frame before each command, on
//! a 960 x 640 surface, and every logged key press applies. Pointer
//! positions map to image pixels through the renderer's own geometry for
//! the logged window size. Movies follow the headless policy: they complete
//! at once instead of playing, so a script's `playmovie` cannot stall the
//! replay.
use crate::{
    AppResult,
    input::{InputCommand, KeyRepeat, LoggedCommand},
    map_preview::MapPreview,
    preview::AssetPreview,
};
use sha2::{Digest, Sha256};
use std::io::BufRead;
use vault_core::{CommandId, host_save::SaveInfo, movie::MoviePolicy};
use winit::keyboard::KeyCode;

/// Simulated milliseconds per command in an untimed log: the desktop's
/// idle redraw interval.
pub const FRAME_MS: u64 = 16;
/// The desktop's initial logical window size, for an untimed log until it
/// reports a resize.
const INITIAL_SURFACE: [u32; 2] = [960, 640];

/// The interactive world a preview shows, if any: a map preview, or the
/// game's current world.
pub fn interactive_world(preview: &mut AssetPreview) -> Option<&mut MapPreview> {
    match preview {
        AssetPreview::Map(map) => Some(&mut **map),
        AssetPreview::Game(game) => game.world(),
        _ => None,
    }
}

/// The desktop shell's vault-debug hook, run after each input event it
/// handles: every command the F9 overlay committed is logged (`log`, which
/// writes the `debug` record) and then applied, in order. A replay of the
/// log applies those records and discards the overlay's outbox, so each
/// command applies exactly once. A refused command is reported, not fatal.
/// Returns whether anything was applied (the frame should be redrawn).
pub fn drain_debug_commands(
    preview: &mut AssetPreview,
    mut log: impl FnMut(&InputCommand) -> AppResult<()>,
) -> AppResult<bool> {
    let Some(world) = interactive_world(preview) else {
        return Ok(false);
    };
    let commands = world.take_debug_commands();
    for command in &commands {
        log(&InputCommand::Debug {
            command: command.clone(),
        })?;
        // Refusals are reported on the overlay and in the log.
        let _ = world.apply_debug(command);
    }
    Ok(!commands.is_empty())
}

/// The replay's result: what was applied and the final state digest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReplayReport {
    /// Commands applied, up to and including a quit.
    pub commands: u64,
    /// Simulated time at the end.
    pub elapsed_ms: u64,
    /// SHA-256 of the final state, lowercase hexadecimal.
    pub state_sha256: String,
}

pub struct Replay {
    preview: AssetPreview,
    surface: [u32; 2],
    image: [u32; 2],
    pointer: Option<[f64; 2]>,
    now: u64,
    commands: u64,
    last: Option<CommandId>,
    stopped: bool,
    /// The log records its frames (it starts with a `surface` record);
    /// `None` until the first record is read.
    timed: Option<bool>,
    /// SPEC-014 §3.29 the engine key repeat, as on the desktop.
    repeat: KeyRepeat,
    /// The logged display scaling (integer until a `scaling` record).
    scaling: vault_render::ScalingMode,
}

impl Replay {
    /// Start from a loaded preview, presented once as the desktop does when
    /// its window opens. A world never waits on movies here.
    pub fn new(mut preview: AssetPreview) -> AppResult<Self> {
        if let AssetPreview::Map(map) = &mut preview {
            map.host.movie_policy = MoviePolicy::CompleteImmediately;
        }
        let mut replay = Self {
            preview,
            surface: INITIAL_SURFACE,
            scaling: vault_render::ScalingMode::Integer,
            image: [0; 2],
            pointer: None,
            now: 0,
            commands: 0,
            last: None,
            stopped: false,
            timed: None,
            repeat: KeyRepeat::default(),
        };
        replay.present()?;
        Ok(replay)
    }

    /// Apply every command of a JSONL log (blank lines are skipped), stopping
    /// at a quit, an Escape a standalone preview does not consume, or the
    /// end of a standalone movie. Command identifiers must increase, and so
    /// must a timed log's frame times.
    pub fn run(&mut self, log: impl BufRead) -> AppResult<()> {
        for (number, line) in log.lines().enumerate() {
            if self.stopped {
                break;
            }
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let logged: LoggedCommand = serde_json::from_str(&line)
                .map_err(|error| format!("replay line {}: {error}", number + 1))?;
            if self.last.is_some_and(|last| logged.command_id <= last) {
                return Err(format!(
                    "replay line {}: command_id {} does not follow {}",
                    number + 1,
                    logged.command_id.get(),
                    self.last.map_or(0, CommandId::get)
                )
                .into());
            }
            self.last = Some(logged.command_id);
            let timed = *self
                .timed
                .get_or_insert(matches!(logged.command, InputCommand::Surface { .. }));
            if !timed {
                self.frame(self.now + FRAME_MS, false)?;
            }
            if let InputCommand::Tick { now_ms } = logged.command
                && now_ms < self.now
            {
                return Err(format!(
                    "replay line {}: tick at {now_ms} ms is before the last frame at {} ms",
                    number + 1,
                    self.now
                )
                .into());
            }
            if !self.stopped {
                self.apply(logged.command)?;
                self.commands += 1;
            }
            // The replayed input may have committed overlay commands; the
            // desktop logged and applied those as their own `debug`
            // records, which this replay applies when it reaches them.
            if let Some(world) = interactive_world(&mut self.preview) {
                world.take_debug_commands();
            }
        }
        Ok(())
    }

    /// One desktop frame at `now`. A timed log follows the desktop's
    /// order: the preview ticks, a finished one stops, a held key repeats,
    /// and the changed frame is presented. An untimed log keeps the order
    /// its simulated frames always had.
    fn frame(&mut self, now: u64, timed: bool) -> AppResult<()> {
        self.now = now;
        let changed = self.preview.tick(now)?;
        if !timed {
            if changed {
                self.present()?;
            }
            self.stopped |= self.preview.finished();
            return Ok(());
        }
        if self.preview.finished() {
            self.stopped = true;
            return Ok(());
        }
        for key in self.repeat.poll(now, self.preview.keyboard()) {
            self.press_key(key, self.preview.escape_exits())?;
        }
        if changed {
            self.present()?;
        }
        Ok(())
    }

    /// A key press reaches the preview; an Escape it does not consume stops
    /// the replay when `exits` (the desktop's standalone previews, or any
    /// preview for an older desktop).
    fn press_key(&mut self, key: KeyCode, exits: bool) -> AppResult<()> {
        if self.preview.step(key) {
            self.present()?;
        } else if key == KeyCode::Escape && exits {
            self.stopped = true;
        }
        Ok(())
    }

    /// Compose the presented frame; dialogue choice areas and the picking
    /// geometry follow it, as on the desktop.
    fn present(&mut self) -> AppResult<()> {
        self.compose()?;
        self.hover();
        Ok(())
    }
    /// Compose the frame, as the desktop does for its renderer.
    fn compose(&mut self) -> AppResult<()> {
        let image = self.preview.image()?;
        self.image = [image.width(), image.height()];
        Ok(())
    }

    /// The pointer's frame pixel through the renderer's viewport for the
    /// logged surface, clamped to the frame as the desktop maps presses,
    /// releases and hover (H3).
    fn pixel(&self) -> Option<[i32; 2]> {
        vault_render::image_position_clamped_with(
            self.surface,
            self.image,
            self.scaling,
            self.pointer?,
        )
    }

    /// Pointer moves reach the preview clamped to the frame, with whether
    /// they are outside it, as on the desktop.
    fn hover(&mut self) {
        if let Some(point) = self.pointer.and_then(|point| {
            vault_render::frame_point_with(self.surface, self.image, self.scaling, point)
        }) {
            self.preview.pointer_moved(point);
        }
    }

    /// The desktop shell's handling of one observed command.
    fn apply(&mut self, command: InputCommand) -> AppResult<()> {
        let timed = self.timed == Some(true);
        match command {
            InputCommand::Key {
                physical_key,
                pressed,
                repeat,
            } => {
                // Unidentified keys reach neither the preview nor Escape.
                let Ok(key) =
                    serde_json::from_value::<KeyCode>(serde_json::Value::String(physical_key))
                else {
                    return Ok(());
                };
                if !timed {
                    // An older desktop applied every press, OS repeats too,
                    // and left at any Escape it did not consume.
                    if pressed {
                        self.press_key(key, true)?;
                    }
                } else if !pressed {
                    self.repeat.release(key);
                    self.preview.key_up(key);
                } else if !repeat {
                    // The desktop ignores the OS repeats; the engine repeats.
                    self.repeat.press(key, self.preview.keyboard());
                    self.press_key(key, self.preview.escape_exits())?;
                }
            }
            InputCommand::Tick { now_ms } => self.frame(now_ms, timed)?,
            InputCommand::Surface { width, height } => {
                self.surface = [width, height];
                // The desktop opening its window again composes the frame
                // (without a hover); the first opening was composed when
                // the replay began.
                if self.commands > 0 {
                    self.compose()?;
                }
            }
            InputCommand::Focus { focused: false } => {
                self.repeat.clear();
                self.preview.focus_lost();
            }
            InputCommand::PointerMoved { x, y } => {
                self.pointer = Some([x, y]);
                self.hover();
            }
            InputCommand::PointerLeft => {
                self.pointer = None;
                self.preview.pointer_left();
            }
            InputCommand::PointerButton { button, pressed } => {
                let right = match button.as_str() {
                    "Left" => false,
                    "Right" => true,
                    _ => return Ok(()),
                };
                if !pressed && self.preview.release(self.pixel(), right) {
                    self.present()?;
                }
                if pressed
                    && let Some(pixel) = self.pixel()
                    && self.preview.click(pixel, right)
                {
                    self.present()?;
                }
            }
            InputCommand::Wheel { y, .. } => {
                if self.preview.scroll(y) {
                    self.present()?;
                }
            }
            // M-P1: the stored pointer is mapped again in the new viewport.
            InputCommand::Resize { width, height } => {
                self.surface = [width, height];
                self.hover();
            }
            InputCommand::Scaling { aspect_fit } => {
                self.scaling = if aspect_fit {
                    vault_render::ScalingMode::AspectFit
                } else {
                    vault_render::ScalingMode::Integer
                };
                self.hover();
            }
            InputCommand::Modifiers {
                shift,
                control,
                alt,
                ..
            } => self.preview.set_modifiers([shift, control, alt]),
            InputCommand::Quit => self.stopped = true,
            // The desktop applied the command right after logging it, then
            // drew the frame.
            InputCommand::Debug { command } => {
                if let Some(world) = interactive_world(&mut self.preview) {
                    world.take_debug_commands();
                    if world.apply_debug(&command).is_ok() || world.debug_enabled() {
                        self.compose()?;
                    }
                }
            }
            // Focus does not reach the preview; a logged hover is the
            // desktop's output, recomputed here.
            InputCommand::Focus { .. } | InputCommand::MapHover { .. } => {}
        }
        Ok(())
    }

    /// Digest the final state: for an interactive world (a map preview or
    /// the game's), what a save slot would hold (central save, working saved
    /// maps, automap); then the presented frame and its display palette. Each
    /// part is length-prefixed. With `frame`, the final frame is also written
    /// there as a binary PPM.
    pub fn finish(mut self, frame: Option<&std::path::Path>) -> AppResult<ReplayReport> {
        let mut digest = Sha256::new();
        let mut part = |bytes: &[u8]| {
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        };
        let world = match &mut self.preview {
            AssetPreview::Map(map) => Some(&mut **map),
            AssetPreview::Game(game) => game.world(),
            _ => None,
        };
        if let Some(map) = world {
            // Building the save first joins the current map to the working
            // set. A central save the grammar cannot encode (such as a map
            // name too long for the manifest) contributes its error instead.
            let central = map
                .host
                .build_save(&SaveInfo::default())
                .and_then(|save| Ok(save.write()?));
            match central {
                Ok(bytes) => part(&bytes),
                Err(error) => {
                    tracing::warn!(%error, "replay digest without a central save");
                    part(error.to_string().as_bytes());
                }
            }
            let persistence = &map.host.world.persistence;
            for (name, bytes) in &persistence.maps {
                part(name.as_bytes());
                part(bytes);
            }
            part(persistence.automap.bytes());
        }
        let image = self.preview.image()?;
        part(&image.width().to_le_bytes());
        part(&image.height().to_le_bytes());
        part(image.pixels());
        let palette = self
            .preview
            .display_override()
            .unwrap_or_else(|| self.preview.palette().rgba8());
        part(palette.as_flattened());
        if let Some(path) = frame {
            let mut ppm = format!("P6\n{} {}\n255\n", image.width(), image.height()).into_bytes();
            for index in image.pixels() {
                ppm.extend_from_slice(&palette[usize::from(*index)][..3]);
            }
            std::fs::write(path, ppm).map_err(|error| format!("{}: {error}", path.display()))?;
        }
        let state_sha256 = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok(ReplayReport {
            commands: self.commands,
            elapsed_ms: self.now,
            state_sha256,
        })
    }
}

/// Replay `log` against `preview` and report the final digest.
pub fn replay(
    preview: AssetPreview,
    log: impl BufRead,
    frame: Option<&std::path::Path>,
) -> AppResult<ReplayReport> {
    let mut replay = Replay::new(preview)?;
    replay.run(log)?;
    replay.finish(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{map_preview::state_fixture, preview::PreviewArgs};

    /// The 960 x 640 window shows the 640 x 480 frame (map and interface
    /// bar) at scale 1, inset 160 x 80. A right-click turns the movement
    /// cursor into the arrow, whose click on the synthetic NPC talks to it;
    /// then its choice is clicked and 1 pressed.
    const DIALOGUE_LOG: &str = r#"{"command_id":1,"kind":"focus","focused":true}
{"command_id":2,"kind":"pointer_moved","x":496.5,"y":250.5}
{"command_id":3,"kind":"pointer_button","button":"Right","pressed":true}
{"command_id":4,"kind":"pointer_button","button":"Right","pressed":false}

{"command_id":5,"kind":"pointer_button","button":"Left","pressed":true}
{"command_id":6,"kind":"pointer_button","button":"Left","pressed":false}
{"command_id":7,"kind":"map_hover","elevation":0,"hex":null,"floor_square":null,"roof_square":null}
{"command_id":8,"kind":"pointer_moved","x":360.5,"y":418.5}
{"command_id":9,"kind":"pointer_button","button":"Left","pressed":true}
{"command_id":10,"kind":"key","physical_key":"Digit1","pressed":true,"repeat":false}
{"command_id":11,"kind":"quit"}
{"command_id":12,"kind":"key","physical_key":"Digit1","pressed":true,"repeat":false}
"#;

    fn args(root: &std::path::Path) -> PreviewArgs {
        PreviewArgs {
            corpus_dir: Some(root.into()),
            preview_map: Some("maps/synthetic.map".into()),
            player_gcd: Some("premade/synthetic.gcd".into()),
            headless: true,
            ..Default::default()
        }
    }

    /// With the Vanilla preset (the default) the dialogue replay and its
    /// idle variant end in the state digests recorded before the Phase 0
    /// shell, debugger and mod work; any change to Vanilla behaviour that
    /// the digest covers fails here. The digests are written in halves.
    #[test]
    fn vanilla_replays_keep_their_recorded_digests() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        let dialogue = replay_log(dir.path(), DIALOGUE_LOG);
        assert_eq!(
            dialogue.state_sha256,
            concat!(
                "1a641c22334423ebdf4a4304581b843d",
                "db3faf99123d32d6cc33e2be7d11ed2c"
            )
        );
        assert_eq!((dialogue.commands, dialogue.elapsed_ms), (11, 176));
        let idle = DIALOGUE_LOG.replace(r#""button":"Right""#, r#""button":"Middle""#);
        let idle = replay_log(dir.path(), &idle);
        assert_eq!(
            idle.state_sha256,
            concat!(
                "9d63926e7cf5003f5b9b59ec4914d98e",
                "48bb9436aa49c5676adea8ad28b0b8af"
            )
        );
    }

    /// A logged `scaling` record maps the pointer through the aspect-fit
    /// viewport (1440 x 1080 at x 240, scale 2.25, on a 1920 x 1080 surface); without
    /// one the replay keeps integer scaling (1280 x 960 at 320, 60).
    #[test]
    fn a_logged_scaling_mode_maps_the_replayed_pointer() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        let log = |scaling: &str| {
            format!(
                "{{\"command_id\":1,\"kind\":\"surface\",\"width\":1920,\"height\":1080}}\n\
                 {scaling}\
                 {{\"command_id\":3,\"kind\":\"pointer_moved\",\"x\":250.5,\"y\":10.5}}\n"
            )
        };
        let mut replay = Replay::new(args(dir.path()).load().unwrap().unwrap()).unwrap();
        replay
            .run(log("{\"command_id\":2,\"kind\":\"scaling\",\"aspect_fit\":true}\n").as_bytes())
            .unwrap();
        assert_eq!(replay.pixel(), Some([4, 4]));
        let mut replay = Replay::new(args(dir.path()).load().unwrap().unwrap()).unwrap();
        replay.run(log("").as_bytes()).unwrap();
        assert_eq!(replay.pixel(), Some([0, 0]));
    }

    #[test]
    fn replays_a_dialogue_log_deterministically_and_stops_at_quit() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        let mut replay = Replay::new(args(dir.path()).load().unwrap().unwrap()).unwrap();
        replay.run(DIALOGUE_LOG.as_bytes()).unwrap();
        let AssetPreview::Map(map) = &replay.preview else {
            panic!("map preview expected");
        };
        // The arrow click opened the conversation; the choices closed it.
        assert!(map.selected.is_some());
        assert!(!map.host.world.dialogue.open);
        assert!(map.host.library.palette_cycling);
        assert_eq!(replay.surface, INITIAL_SURFACE);
        let first = replay.finish(None).unwrap();
        assert_eq!((first.commands, first.elapsed_ms), (11, 11 * FRAME_MS));
        assert_eq!(first.state_sha256.len(), 64);
        let again = replay_log(dir.path(), DIALOGUE_LOG);
        assert_eq!(again, first, "same log, same digest");
        // Without the conversation the final state differs.
        let idle = DIALOGUE_LOG.replace(r#""button":"Right""#, r#""button":"Middle""#);
        let idle = replay_log(dir.path(), &idle);
        assert_eq!(idle.commands, first.commands);
        assert_ne!(idle.state_sha256, first.state_sha256);
    }

    fn replay_log(root: &std::path::Path, log: &str) -> ReplayReport {
        replay(args(root).load().unwrap().unwrap(), log.as_bytes(), None).unwrap()
    }

    /// The fixture's item prototype (a container).
    const ITEM_PID: u32 = 1;

    fn key(id: u64, key: &str) -> String {
        format!(
            r#"{{"command_id":{id},"kind":"key","physical_key":"{key}","pressed":true,"repeat":false}}"#
        )
    }
    /// What the desktop logs when the overlay's spawner commits one item:
    /// F9 opens the overlay, F8 five times reaches the spawner, the PID
    /// is typed and Enter commits; the shell then logs the committed
    /// command as its own record.
    fn spawn_log(with_record: bool) -> String {
        let mut lines = vec![key(1, "F9")];
        for id in 2..=6 {
            lines.push(key(id, "F8"));
        }
        lines.push(key(7, "Digit1"));
        lines.push(key(8, "Enter"));
        if with_record {
            lines.push(format!(
                r#"{{"command_id":9,"kind":"debug","command":{{"op":"spawn_item","pid":{ITEM_PID},"quantity":1}}}}"#
            ));
        }
        lines.join("\n") + "\n"
    }
    /// The state fixture with its map also saved under an 8.3 name, so a
    /// central save can be built and the digest covers the player's
    /// inventory and the campaign variables.
    fn debug_corpus() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        std::fs::copy(
            dir.path().join("maps/synthetic.map"),
            dir.path().join("maps/synth.map"),
        )
        .unwrap();
        dir
    }
    fn debug_args(root: &std::path::Path) -> PreviewArgs {
        PreviewArgs {
            preview_map: Some("maps/synth.map".into()),
            ..args(root)
        }
    }
    fn debug_replay(root: &std::path::Path, log: &str) -> ReplayReport {
        replay(
            debug_args(root).load().unwrap().unwrap(),
            log.as_bytes(),
            None,
        )
        .unwrap()
    }
    fn carried(replay: &mut Replay) -> i32 {
        let world = interactive_world(&mut replay.preview).unwrap();
        let player = world.host.world.player;
        world.host.world.inventory_count(player, ITEM_PID)
    }

    #[test]
    fn logged_debug_commands_apply_once_and_replay_identically() {
        let dir = debug_corpus();
        let start = {
            let mut replay = Replay::new(debug_args(dir.path()).load().unwrap().unwrap()).unwrap();
            carried(&mut replay)
        };
        // The overlay's own commit in a replay is discarded; only the
        // logged record applies, so the item is created exactly once.
        let mut replay = Replay::new(debug_args(dir.path()).load().unwrap().unwrap()).unwrap();
        replay.run(spawn_log(true).as_bytes()).unwrap();
        assert_eq!(carried(&mut replay), start + 1);
        assert!(
            interactive_world(&mut replay.preview)
                .unwrap()
                .take_debug_commands()
                .is_empty()
        );
        let first = replay.finish(None).unwrap();
        assert_eq!(first, debug_replay(dir.path(), &spawn_log(true)));
        // Without the record nothing is created, and the digest differs.
        let mut replay = Replay::new(debug_args(dir.path()).load().unwrap().unwrap()).unwrap();
        replay.run(spawn_log(false).as_bytes()).unwrap();
        assert_eq!(carried(&mut replay), start);
        assert_ne!(
            replay.finish(None).unwrap().state_sha256,
            first.state_sha256
        );

        // A record applies with the overlay closed too, and a refused one
        // changes nothing.
        let log = [
            r#"{"command_id":1,"kind":"debug","command":{"op":"set_global_var","index":0,"value":4242}}"#,
            r#"{"command_id":2,"kind":"debug","command":{"op":"set_global_var","index":999999,"value":1}}"#,
            r#"{"command_id":3,"kind":"debug","command":{"op":"spawn_item","pid":1,"quantity":2}}"#,
        ]
        .join("\n");
        let mut replay = Replay::new(debug_args(dir.path()).load().unwrap().unwrap()).unwrap();
        replay.run(log.as_bytes()).unwrap();
        assert_eq!(carried(&mut replay), start + 2);
        let world = interactive_world(&mut replay.preview).unwrap();
        assert_eq!(world.host.world.gvars.as_ref().unwrap()[0], 4242);
        assert!(!world.debug_enabled());
        let once = replay.finish(None).unwrap();
        assert_eq!(once.commands, 3);
        assert_eq!(
            once,
            debug_replay(dir.path(), &log),
            "same log, same digest"
        );
    }

    #[test]
    fn the_desktop_hook_logs_then_applies_and_its_log_replays_the_same_state() {
        let dir = debug_corpus();
        let mut desktop = debug_args(dir.path()).load().unwrap().unwrap();
        let mut ids = vault_core::SequenceIds::default();
        let mut log = Vec::new();
        let mut write = |log: &mut Vec<u8>, command: &InputCommand| {
            crate::input::write_command(log, &mut ids, command)
        };
        let keys = [
            KeyCode::F9,
            KeyCode::F8,
            KeyCode::F8,
            KeyCode::Digit7,
            KeyCode::Enter,
        ];
        for key in keys {
            let physical_key = serde_json::to_value(key)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            write(
                &mut log,
                &InputCommand::Key {
                    physical_key,
                    pressed: true,
                    repeat: false,
                },
            )
            .unwrap();
            desktop.tick(0).unwrap();
            desktop.step(key);
            drain_debug_commands(&mut desktop, |command| write(&mut log, command)).unwrap();
        }
        let text = String::from_utf8(log).unwrap();
        assert!(
            text.contains(
                r#""kind":"debug","command":{"op":"set_global_var","index":0,"value":7}"#
            ),
            "{text}"
        );
        let world = interactive_world(&mut desktop).unwrap();
        assert_eq!(world.host.world.gvars.as_ref().unwrap()[0], 7);
        assert!(world.take_debug_commands().is_empty(), "drained");
        let mut replay = Replay::new(debug_args(dir.path()).load().unwrap().unwrap()).unwrap();
        replay.run(text.as_bytes()).unwrap();
        let world = interactive_world(&mut replay.preview).unwrap();
        assert_eq!(world.host.world.gvars.as_ref().unwrap()[0], 7);
        assert!(world.debug_enabled());
    }

    #[test]
    fn resizes_move_the_picking_geometry_and_bad_logs_fail() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        // At 1920 x 1280 the frame is drawn at scale 2, inset 320 x 160: the
        // NPC's window position moves accordingly.
        let log = r#"{"command_id":1,"kind":"resize","width":1920,"height":1280}
{"command_id":2,"kind":"pointer_moved","x":993.0,"y":501.0}
{"command_id":3,"kind":"pointer_button","button":"Right","pressed":true}
{"command_id":4,"kind":"pointer_button","button":"Left","pressed":true}
{"command_id":5,"kind":"pointer_button","button":"Left","pressed":false}
"#;
        let mut replay = Replay::new(args(dir.path()).load().unwrap().unwrap()).unwrap();
        replay.run(log.as_bytes()).unwrap();
        assert_eq!(replay.pixel(), Some([336, 170]));
        let AssetPreview::Map(map) = &replay.preview else {
            panic!("map preview expected");
        };
        assert!(map.selected.is_some());
        // H3: presses in the letterbox map to the nearest frame pixel, as
        // hover does.
        replay.pointer = Some([10.0, 1270.0]);
        assert_eq!(replay.pixel(), Some([0, 479]));
        let mut replay = Replay::new(args(dir.path()).load().unwrap().unwrap()).unwrap();
        // Identifiers must increase: a repeated one is a merged or edited log.
        let repeated = r#"{"command_id":2,"kind":"focus","focused":true}
{"command_id":2,"kind":"focus","focused":false}
"#;
        let error = replay.run(repeated.as_bytes()).unwrap_err();
        assert!(error.to_string().contains("line 2"), "{error}");
        let mut replay = Replay::new(args(dir.path()).load().unwrap().unwrap()).unwrap();
        assert!(
            replay
                .run(&b"{\"command_id\":1,\"kind\":\"jump\"}\n"[..])
                .is_err()
        );
    }

    #[test]
    fn timed_logs_replay_their_surface_frames_and_key_repeats() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        // D-11: the log opens with the physical surface (a HiDPI window of
        // 1920 x 1280 draws the frame at scale 2, inset 320 x 160), and its
        // ticks are the only clock. A right-click turns the cursor into the
        // arrow over the NPC and the left button goes down at 32 ms; the
        // action menu opens on the first frame more than 250 ms later
        // (SPEC-014 §3.28).
        let hold = |last: u64| {
            format!(
                r#"{{"command_id":1,"kind":"surface","width":1920,"height":1280}}
{{"command_id":2,"kind":"tick","now_ms":16}}
{{"command_id":3,"kind":"pointer_moved","x":993.0,"y":501.0}}
{{"command_id":4,"kind":"tick","now_ms":32}}
{{"command_id":5,"kind":"pointer_button","button":"Right","pressed":true}}
{{"command_id":6,"kind":"pointer_button","button":"Left","pressed":true}}
{{"command_id":7,"kind":"tick","now_ms":{last}}}
"#
            )
        };
        let menu = |last: u64| {
            let mut replay = Replay::new(args(dir.path()).load().unwrap().unwrap()).unwrap();
            replay.run(hold(last).as_bytes()).unwrap();
            assert_eq!(replay.pixel(), Some([336, 170]));
            assert_eq!((replay.now, replay.commands), (last, 7));
            let AssetPreview::Map(map) = &replay.preview else {
                panic!("map preview expected");
            };
            map.menu_open()
        };
        assert!(!menu(282), "250 ms is not more than 250");
        assert!(menu(283));
        // SPEC-014 §3.29: a held arrow repeats on the logged frames, more
        // than 500 ms after the frame that saw the press (32 ms), while the
        // OS repeat is ignored: two scroll steps in all.
        let keys = r#"{"command_id":1,"kind":"surface","width":960,"height":640}
{"command_id":2,"kind":"tick","now_ms":16}
{"command_id":3,"kind":"key","physical_key":"ArrowRight","pressed":true,"repeat":false}
{"command_id":4,"kind":"tick","now_ms":32}
{"command_id":5,"kind":"tick","now_ms":532}
{"command_id":6,"kind":"key","physical_key":"ArrowRight","pressed":true,"repeat":true}
{"command_id":7,"kind":"tick","now_ms":548}
{"command_id":8,"kind":"tick","now_ms":564}
{"command_id":9,"kind":"key","physical_key":"ArrowRight","pressed":false,"repeat":false}
{"command_id":10,"kind":"tick","now_ms":2000}
"#;
        let mut replay = Replay::new(args(dir.path()).load().unwrap().unwrap()).unwrap();
        replay.run(keys.as_bytes()).unwrap();
        let AssetPreview::Map(map) = &replay.preview else {
            panic!("map preview expected");
        };
        let Some(AssetPreview::Map(mut desktop)) = args(dir.path()).load().unwrap() else {
            panic!("map preview expected");
        };
        let start = desktop.host.world.camera_hex;
        for now in [16, 548] {
            desktop.tick(now).unwrap();
            assert!(desktop.step(KeyCode::ArrowRight));
        }
        assert_ne!(desktop.host.world.camera_hex, start);
        assert_eq!(map.host.world.camera_hex, desktop.host.world.camera_hex);
        assert_eq!(replay.now, 2000);
        // Frame times must not go back.
        let mut replay = Replay::new(args(dir.path()).load().unwrap().unwrap()).unwrap();
        let back = r#"{"command_id":1,"kind":"surface","width":960,"height":640}
{"command_id":2,"kind":"tick","now_ms":50}
{"command_id":3,"kind":"tick","now_ms":40}
"#;
        let error = replay.run(back.as_bytes()).unwrap_err();
        assert!(error.to_string().contains("line 3"), "{error}");
    }

    #[test]
    fn headless_worlds_complete_movies_instead_of_waiting_for_them() {
        let dir = tempfile::tempdir().unwrap();
        state_fixture::write_corpus(dir.path());
        crate::movie::tests::movie_corpus(dir.path());
        let mut desktop = args(dir.path());
        desktop.startup_movies = true;
        desktop.headless = false;
        let Some(AssetPreview::Map(map)) = desktop.load().unwrap() else {
            panic!("map preview expected");
        };
        assert!(map.movie_active(), "the desktop presents the intro");
        assert_eq!(map.host.movie_policy, MoviePolicy::WaitUntilFinished);
        desktop.headless = true;
        let Some(AssetPreview::Map(mut map)) = desktop.load().unwrap() else {
            panic!("map preview expected");
        };
        assert!(!map.movie_active(), "headless: nothing to wait for");
        assert_eq!(
            map.host.world.persistence.movies[vault_core::movie::INTRO],
            1
        );
        // Later requests complete too, and the world keeps running.
        assert!(map.host.play_game_movie(vault_core::movie::INTRO, 0));
        let ticks = map.host.world.clock.ticks;
        map.tick(1000).unwrap();
        assert!(!map.movie_active());
        assert!(map.host.world.clock.ticks > ticks);
    }
}
