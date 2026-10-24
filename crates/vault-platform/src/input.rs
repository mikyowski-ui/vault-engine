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

/// The keyboard state the key repeat reads from the game at each poll
/// (SPEC-014 §3.29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keyboard {
    /// Input is polled and the keyboard is enabled: not during a game fade
    /// (§3.2) or while combat disables the keyboard (§3.27).
    pub enabled: bool,
    /// How many times the game has cleared the keyboard (each player
    /// combat turn start and each main-menu loop pass); any change cancels
    /// the repeat of every held key.
    pub clears: u64,
}
impl Default for Keyboard {
    fn default() -> Self {
        Self {
            enabled: true,
            clears: 0,
        }
    }
}

/// SPEC-014 §3.29 keyboard auto-repeat, produced by the game from the
/// shell clock instead of the operating system's repeat. At each poll with
/// the keyboard enabled every key still held makes one more press, when
/// more than 500 ms have passed since its press or more than 80 ms since
/// its previous repeat. Every physical key repeats alike, window toggles
/// included (F1, Tab, Esc and the window hotkeys): a held toggle opens its
/// window, its repeats then reach that window, which may close it, and so
/// on until the key is released. Clearing the keyboard cancels every held
/// key's repeat until that key is pressed again.
///
/// Times are frame times: a press counts from the first poll after it, so
/// no repeat comes early, and a replay of the logged frames repeats as the
/// desktop did. Modifier and lock keys repeat by the same rule; each press
/// or repeat is a key event of its own, but it translates to no key code
/// ([`is_modifier`]).
#[derive(Debug, Default)]
pub struct KeyRepeat {
    /// The held keys, in press order.
    held: Vec<HeldKey>,
    /// The keyboard clears last seen.
    clears: u64,
}
#[derive(Debug, Clone, Copy)]
struct HeldKey {
    key: winit::keyboard::KeyCode,
    /// The poll after the press (`None` until it comes), then the last
    /// repeat.
    last: Option<u64>,
    repeating: bool,
}
impl KeyRepeat {
    /// A clear since the last look cancels every held key.
    fn sync(&mut self, keyboard: Keyboard) {
        if keyboard.clears != self.clears {
            self.clears = keyboard.clears;
            self.held.clear();
        }
    }
    /// A key went down (before the press was handled).
    pub fn press(&mut self, key: winit::keyboard::KeyCode, keyboard: Keyboard) {
        self.sync(keyboard);
        self.held.retain(|held| held.key != key);
        self.held.push(HeldKey {
            key,
            last: None,
            repeating: false,
        });
    }
    /// A key went up: it stops repeating.
    pub fn release(&mut self, key: winit::keyboard::KeyCode) {
        self.held.retain(|held| held.key != key);
    }
    /// The window lost the keyboard: nothing repeats.
    pub fn clear(&mut self) {
        self.held.clear();
    }
    /// The poll at `now`: the held keys whose repeat is due, in press
    /// order. Nothing repeats while the keyboard is disabled.
    pub fn poll(&mut self, now: u64, keyboard: Keyboard) -> Vec<winit::keyboard::KeyCode> {
        self.sync(keyboard);
        let mut due = Vec::new();
        for held in &mut self.held {
            let Some(last) = held.last else {
                held.last = Some(now);
                continue;
            };
            let wait = if held.repeating {
                REPEAT_INTERVAL_MS
            } else {
                REPEAT_DELAY_MS
            };
            if !keyboard.enabled || now.saturating_sub(last) <= wait {
                continue;
            }
            held.last = Some(now);
            held.repeating = true;
            due.push(held.key);
        }
        due
    }
}

/// SPEC-014 §3.29: Shift, Ctrl, Alt, Caps Lock, Num Lock and Scroll Lock
/// (and the Windows keys) are physical keys like any other: they are timed
/// and repeated, and each press or repeat takes one slot of the 32-event
/// keyboard buffer (§3.27). A buffered event is translated to a key code
/// only when it is read, with the modifiers held when it was typed, and a
/// modifier key on its own translates to none: its event is removed and
/// nothing acts, so no window opens or closes and no "any key" test is
/// met. Holding Ctrl never closes the pause window Ctrl+P opened, while P
/// held with Ctrl repeats Ctrl+P like any other key.
pub fn is_modifier(key: winit::keyboard::KeyCode) -> bool {
    use winit::keyboard::KeyCode::*;
    matches!(
        key,
        ShiftLeft
            | ShiftRight
            | ControlLeft
            | ControlRight
            | AltLeft
            | AltRight
            | SuperLeft
            | SuperRight
            | CapsLock
            | NumLock
            | ScrollLock
    )
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

/// SPEC-014 §3.18 `mouse_sensitivity` (1.0 through 2.5) for the drawn
/// software cursor. The spec gives no formula; this engine scales the OS
/// pointer's motion by it while the game draws its own cursor, and uses
/// the OS absolute position unchanged whenever the system cursor shows, at
/// sensitivity 1.0, or when the pointer comes back into the window.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct VirtualPointer {
    /// The last OS absolute position.
    os: Option<[f64; 2]>,
    /// The pointer the game uses (surface pixels).
    at: Option<[f64; 2]>,
}
impl VirtualPointer {
    /// The OS pointer moved to `os`. With the software cursor at a
    /// sensitivity above 1.0 the pointer moves by the OS delta times
    /// `scale`, kept inside the `surface`; otherwise it is `os` itself.
    pub fn moved(
        &mut self,
        os: [f64; 2],
        scale: f64,
        software: bool,
        surface: [f64; 2],
    ) -> [f64; 2] {
        let at = match (self.os, self.at) {
            (Some(last), Some(at)) if software && scale > 1.0 => {
                let limit =
                    |axis: usize, value: f64| value.clamp(0.0, (surface[axis] - 1.0).max(0.0));
                [
                    limit(0, at[0] + (os[0] - last[0]) * scale),
                    limit(1, at[1] + (os[1] - last[1]) * scale),
                ]
            }
            _ => os,
        };
        self.os = Some(os);
        self.at = Some(at);
        at
    }
    /// The pointer left the window: the next position re-anchors on the OS.
    pub fn left(&mut self) {
        *self = Self::default();
    }
    /// The OS pointer was moved to the game's pointer (a warp), so both
    /// agree again.
    pub fn anchor(&mut self, point: [f64; 2]) {
        self.os = Some(point);
        self.at = Some(point);
    }
    /// The game's pointer is away from the hidden OS pointer by more than
    /// half a pixel, so the OS pointer should follow it (a warp) before it
    /// can leave the window on its own.
    pub fn drifted(&self) -> Option<[f64; 2]> {
        let (os, at) = (self.os?, self.at?);
        ((os[0] - at[0]).abs() > 0.5 || (os[1] - at[1]).abs() > 0.5).then_some(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_sensitivity_scales_software_cursor_motion_only() {
        let surface = [960.0, 640.0];
        let mut pointer = VirtualPointer::default();
        // The first position, and the system cursor, are the OS position.
        assert_eq!(
            pointer.moved([100.0, 100.0], 2.0, true, surface),
            [100.0, 100.0]
        );
        assert_eq!(
            pointer.moved([110.0, 95.0], 2.0, true, surface),
            [120.0, 90.0]
        );
        assert_eq!(pointer.drifted(), Some([120.0, 90.0]));
        pointer.anchor([120.0, 90.0]);
        assert_eq!(pointer.drifted(), None);
        assert_eq!(
            pointer.moved([130.0, 90.0], 2.0, false, surface),
            [130.0, 90.0]
        );
        assert_eq!(
            pointer.moved([140.0, 90.0], 1.0, true, surface),
            [140.0, 90.0]
        );
        // Scaled motion stays inside the surface.
        assert_eq!(
            pointer.moved([600.0, 0.0], 2.5, true, surface),
            [959.0, 0.0]
        );
        // Leaving the window re-anchors on the OS position.
        pointer.left();
        assert_eq!(pointer, VirtualPointer::default());
        assert_eq!(pointer.moved([5.0, 6.0], 2.5, true, surface), [5.0, 6.0]);
    }

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
        let on = Keyboard::default();
        let mut repeat = KeyRepeat::default();
        repeat.press(KeyCode::ArrowUp, on);
        // Frames every 16 ms, the first after the press at 1000: the first
        // repeat comes on the first frame more than 500 ms later (512 ms),
        // the next on the first frame more than 80 ms after it (96 ms).
        let mut fired = Vec::new();
        let mut now = 984;
        while now < 1800 {
            now += 16;
            for key in repeat.poll(now, on) {
                assert_eq!(key, KeyCode::ArrowUp);
                fired.push(now);
            }
        }
        assert_eq!(fired[0], 1512, "500 ms is not yet more than 500");
        assert_eq!(fired[1] - fired[0], 96, "the first frame past 80 ms");
        repeat.release(KeyCode::ArrowUp);
        // Exactly 500 ms does not repeat.
        repeat.press(KeyCode::ArrowLeft, on);
        assert!(repeat.poll(0, on).is_empty());
        assert!(repeat.poll(500, on).is_empty());
        assert_eq!(repeat.poll(501, on), [KeyCode::ArrowLeft]);
        assert!(repeat.poll(581, on).is_empty());
        assert_eq!(repeat.poll(582, on), [KeyCode::ArrowLeft]);
        // Every held key repeats, each on its own clock, in press order.
        repeat.press(KeyCode::KeyA, on);
        assert!(repeat.poll(600, on).is_empty());
        assert_eq!(repeat.poll(663, on), [KeyCode::ArrowLeft]);
        assert_eq!(repeat.poll(1101, on), [KeyCode::ArrowLeft, KeyCode::KeyA]);
        // Releasing another key changes nothing; releasing it stops it.
        repeat.release(KeyCode::KeyA);
        assert_eq!(repeat.poll(1200, on), [KeyCode::ArrowLeft]);
        repeat.release(KeyCode::ArrowLeft);
        assert!(repeat.poll(2000, on).is_empty());
        // Fixture S: Tab held from 0 ms with polls every 41 ms repeats at
        // 533, 615 and 697 ms: window toggles repeat like any key.
        repeat.press(KeyCode::Tab, on);
        let fired: Vec<u64> = (0..18)
            .map(|poll| poll * 41)
            .filter(|now| !repeat.poll(*now, on).is_empty())
            .collect();
        assert_eq!(fired, [533, 615, 697]);
        // No repeat while the keyboard is disabled; the next enabled poll
        // past the interval repeats.
        let off = Keyboard {
            enabled: false,
            ..on
        };
        assert!(repeat.poll(1000, off).is_empty());
        assert_eq!(repeat.poll(1001, on), [KeyCode::Tab]);
        // Clearing the keyboard (a player turn start, a main-menu pass)
        // cancels the repeat until the key is pressed again.
        let cleared = Keyboard { clears: 1, ..on };
        assert!(repeat.poll(3000, cleared).is_empty());
        assert!(repeat.poll(5000, cleared).is_empty());
        repeat.press(KeyCode::Tab, cleared);
        assert!(repeat.poll(5000, cleared).is_empty());
        assert_eq!(repeat.poll(5501, cleared), [KeyCode::Tab]);
        // A clear between a press and the next poll cancels it too.
        repeat.press(KeyCode::Escape, cleared);
        assert!(repeat.poll(6000, Keyboard { clears: 2, ..on }).is_empty());
        assert!(repeat.poll(7000, Keyboard { clears: 2, ..on }).is_empty());
        // Focus loss stops a repeat.
        repeat.press(KeyCode::ArrowDown, on);
        repeat.clear();
        assert!(repeat.poll(9000, on).is_empty());
    }

    #[test]
    fn spec014_modifier_and_lock_keys_repeat_like_any_key() {
        use winit::keyboard::KeyCode;
        let on = Keyboard::default();
        let mut repeat = KeyRepeat::default();
        // Ctrl held from 0 ms with polls every 41 ms repeats as Tab does in
        // Fixture S, and so does a lock key held beside it.
        repeat.press(KeyCode::ControlLeft, on);
        repeat.press(KeyCode::CapsLock, on);
        let fired: Vec<(u64, Vec<KeyCode>)> = (0..18)
            .map(|poll| poll * 41)
            .map(|now| (now, repeat.poll(now, on)))
            .filter(|(_, due)| !due.is_empty())
            .collect();
        let both = vec![KeyCode::ControlLeft, KeyCode::CapsLock];
        assert_eq!(
            fired,
            [(533, both.clone()), (615, both.clone()), (697, both)]
        );
        // They translate to no key code; the keys they modify do.
        for key in [
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::CapsLock,
            KeyCode::NumLock,
            KeyCode::ScrollLock,
        ] {
            assert!(is_modifier(key), "{key:?}");
        }
        assert!(!is_modifier(KeyCode::KeyP));
        assert!(!is_modifier(KeyCode::Escape));
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
