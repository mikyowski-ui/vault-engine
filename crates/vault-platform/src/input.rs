use std::io::Write;

use serde::{Deserialize, Serialize};
use vault_core::{CommandId, SequenceIds};

use crate::AppResult;

/// Platform observations only; game-specific input bindings belong to later phases.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputCommand {
    Key {
        physical_key: String,
        pressed: bool,
        repeat: bool,
    },
    PointerMoved {
        x: f64,
        y: f64,
    },
    PointerButton {
        button: String,
        pressed: bool,
    },
    /// The pointer left the window.
    PointerLeft,
    Wheel {
        x: f64,
        y: f64,
        unit: WheelUnit,
    },
    Focus {
        focused: bool,
    },
    Modifiers {
        shift: bool,
        control: bool,
        alt: bool,
        super_key: bool,
    },
    Resize {
        width: u32,
        height: u32,
    },
    MapHover {
        elevation: u8,
        hex: Option<i32>,
        floor_square: Option<i32>,
        roof_square: Option<i32>,
    },
    Quit,
    /// One desktop frame: the shell clock, in milliseconds since the
    /// window opened, at which the preview ticked. A replay ticks at
    /// exactly these times.
    Tick {
        now_ms: u64,
    },
    /// The window's inner size in physical pixels when it opened (first
    /// in a log), before any resize.
    Surface {
        width: u32,
        height: u32,
    },
}

/// SPEC-014 §3.29: key auto-repeat starts after more than 500 ms...
pub const REPEAT_DELAY_MS: u64 = 500;
/// ...then repeats every more than 80 ms.
pub const REPEAT_INTERVAL_MS: u64 = 80;

/// SPEC-014 §3.29 keyboard auto-repeat, driven by the shell clock instead
/// of the operating system's repeat: the last key pressed repeats once
/// more than 500 ms have passed since its press, then each time more than
/// 80 ms have passed since the previous repeat, until it is released.
/// Modifier keys neither repeat nor interrupt a repeat. Times are frame
/// times: a press counts from the first frame after it, so no repeat comes
/// early, and a replay of the logged frames repeats as the desktop did.
///
/// A repeat goes only to the input layer `L` the key was pressed in (the
/// game stage, the world's window, conversation or dialog box): once the
/// press or a repeat opens or closes a window, the key stops repeating
/// until it is pressed again, so window-toggle keys (F1, Tab, Escape,
/// Ctrl+P and the window hotkeys) never re-toggle.
#[derive(Debug)]
pub struct KeyRepeat<L> {
    held: Option<HeldKey<L>>,
}
#[derive(Debug, Clone, Copy)]
struct HeldKey<L> {
    key: winit::keyboard::KeyCode,
    layer: L,
    /// The frame of the press (`None` until the first frame after it), then
    /// of the last repeat.
    last: Option<u64>,
    repeating: bool,
}
impl<L> Default for KeyRepeat<L> {
    fn default() -> Self {
        Self { held: None }
    }
}
impl<L: Copy + PartialEq> KeyRepeat<L> {
    /// A key went down in `layer` (before the press was handled).
    pub fn press(&mut self, key: winit::keyboard::KeyCode, layer: L) {
        use winit::keyboard::KeyCode::*;
        if matches!(
            key,
            ShiftLeft
                | ShiftRight
                | ControlLeft
                | ControlRight
                | AltLeft
                | AltRight
                | SuperLeft
                | SuperRight
        ) {
            return;
        }
        self.held = Some(HeldKey {
            key,
            layer,
            last: None,
            repeating: false,
        });
    }
    /// A key went up: it stops repeating.
    pub fn release(&mut self, key: winit::keyboard::KeyCode) {
        if self.held.as_ref().is_some_and(|held| held.key == key) {
            self.held = None;
        }
    }
    /// The window lost the keyboard: nothing repeats.
    pub fn clear(&mut self) {
        self.held = None;
    }
    /// The frame at `now` with the input in `layer`: the key to repeat, if
    /// one is due.
    pub fn poll(&mut self, now: u64, layer: L) -> Option<winit::keyboard::KeyCode> {
        let held = self.held.as_mut()?;
        if held.layer != layer {
            self.held = None;
            return None;
        }
        let Some(last) = held.last else {
            held.last = Some(now);
            return None;
        };
        let wait = if held.repeating {
            REPEAT_INTERVAL_MS
        } else {
            REPEAT_DELAY_MS
        };
        if now.saturating_sub(last) <= wait {
            return None;
        }
        held.last = Some(now);
        held.repeating = true;
        Some(held.key)
    }
}

/// How a wheel delta is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelUnit {
    Lines,
    Pixels,
}

#[derive(Serialize)]
struct Record<'a> {
    command_id: CommandId,
    #[serde(flatten)]
    command: &'a InputCommand,
}

/// One logged command, as read back for a replay.
#[derive(Debug, Deserialize)]
pub struct LoggedCommand {
    pub command_id: CommandId,
    #[serde(flatten)]
    pub command: InputCommand,
}

/// One JSON object per line, without timestamps or debug-formatted payloads.
/// The caller flushes (the shell once per event-loop pass, P-02).
pub fn write_command(
    mut writer: impl Write,
    ids: &mut SequenceIds,
    command: &InputCommand,
) -> AppResult<()> {
    let record = Record {
        command_id: ids.next_command()?,
        command,
    };
    serde_json::to_writer(&mut writer, &record)?;
    writeln!(writer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_log_has_stable_ids_and_typed_json_fields() {
        let mut output = Vec::new();
        let mut ids = SequenceIds::default();
        for command in [
            InputCommand::Key {
                physical_key: "KeyA".into(),
                pressed: true,
                repeat: false,
            },
            InputCommand::PointerMoved { x: 10.5, y: 20.0 },
            InputCommand::Quit,
        ] {
            write_command(&mut output, &mut ids, &command).unwrap();
        }
        assert_eq!(
            String::from_utf8(output).unwrap(),
            include_str!("../../../tests/fixtures/input-commands.jsonl")
        );
    }

    #[test]
    fn keys_repeat_after_more_than_500_ms_then_more_than_80_ms() {
        use winit::keyboard::KeyCode;
        let mut repeat = KeyRepeat::default();
        repeat.press(KeyCode::ArrowUp, 0);
        // Frames every 16 ms, the first after the press at 1000: the first
        // repeat comes on the first frame more than 500 ms later (512 ms),
        // the next on the first frame more than 80 ms after it (96 ms).
        let mut fired = Vec::new();
        let mut now = 984;
        while now < 1800 {
            now += 16;
            if let Some(key) = repeat.poll(now, 0) {
                assert_eq!(key, KeyCode::ArrowUp);
                fired.push(now);
            }
        }
        assert_eq!(fired[0], 1512, "500 ms is not yet more than 500");
        assert_eq!(fired[1] - fired[0], 96, "the first frame past 80 ms");
        // Exactly 500 ms does not repeat; a modifier does not interrupt.
        repeat.press(KeyCode::ArrowLeft, 0);
        assert_eq!(repeat.poll(0, 0), None);
        repeat.press(KeyCode::ShiftLeft, 0);
        assert_eq!(repeat.poll(500, 0), None);
        assert_eq!(repeat.poll(501, 0), Some(KeyCode::ArrowLeft));
        assert_eq!(repeat.poll(581, 0), None);
        assert_eq!(repeat.poll(582, 0), Some(KeyCode::ArrowLeft));
        // Releasing another key changes nothing; releasing it stops it.
        repeat.release(KeyCode::ArrowUp);
        assert_eq!(repeat.poll(700, 0), Some(KeyCode::ArrowLeft));
        repeat.release(KeyCode::ArrowLeft);
        assert_eq!(repeat.poll(2000, 0), None);
        // A press that opened a window (its layer changed) never repeats,
        // even back in the first layer.
        repeat.press(KeyCode::Tab, 0);
        assert_eq!(repeat.poll(0, 1), None);
        assert_eq!(repeat.poll(700, 0), None);
        // Focus loss stops a repeat.
        repeat.press(KeyCode::ArrowDown, 0);
        assert_eq!(repeat.poll(0, 0), None);
        repeat.clear();
        assert_eq!(repeat.poll(600, 0), None);
    }

    #[test]
    fn frame_records_read_back() {
        let tick: LoggedCommand =
            serde_json::from_str(r#"{"command_id":9,"kind":"tick","now_ms":1234}"#).unwrap();
        assert_eq!(tick.command, InputCommand::Tick { now_ms: 1234 });
        let mut output = Vec::new();
        let mut ids = SequenceIds::default();
        let surface = InputCommand::Surface {
            width: 1920,
            height: 1280,
        };
        write_command(&mut output, &mut ids, &surface).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "{\"command_id\":1,\"kind\":\"surface\",\"width\":1920,\"height\":1280}\n"
        );
    }

    #[test]
    fn logged_commands_read_back_with_their_ids() {
        let log = include_str!("../../../tests/fixtures/input-commands.jsonl");
        let commands: Vec<LoggedCommand> = log
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[1].command_id.get(), 2);
        assert_eq!(
            commands[1].command,
            InputCommand::PointerMoved { x: 10.5, y: 20.0 }
        );
        let wheel: LoggedCommand = serde_json::from_str(
            r#"{"command_id":4,"kind":"wheel","x":0.0,"y":-1.0,"unit":"lines"}"#,
        )
        .unwrap();
        assert_eq!(
            wheel.command,
            InputCommand::Wheel {
                x: 0.0,
                y: -1.0,
                unit: WheelUnit::Lines
            }
        );
    }
}
