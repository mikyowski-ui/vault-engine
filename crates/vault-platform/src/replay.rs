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
    preview::{AssetPreview, KeyLayer},
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
    repeat: KeyRepeat<KeyLayer>,
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
        if let Some(key) = self.repeat.poll(now, self.preview.key_layer()) {
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

    fn pixel(&self) -> Option<[i32; 2]> {
        vault_render::image_position(self.surface, self.image, self.pointer?)
    }

    /// Pointer moves reach the preview clamped to the frame, as on the
    /// desktop.
    fn hover(&mut self) {
        if let Some(pixel) = self
            .pointer
            .and_then(|point| vault_render::image_position_clamped(self.surface, self.image, point))
        {
            self.preview.hover(pixel);
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
                    self.repeat.press(key, self.preview.key_layer());
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
            InputCommand::Focus { focused: false } => self.repeat.clear(),
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
            InputCommand::Resize { width, height } => self.surface = [width, height],
            InputCommand::Modifiers {
                shift,
                control,
                alt,
                ..
            } => self.preview.set_modifiers([shift, control, alt]),
            InputCommand::Quit => self.stopped = true,
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
