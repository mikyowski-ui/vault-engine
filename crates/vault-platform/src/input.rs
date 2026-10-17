use std::io::Write;

use serde::Serialize;
use vault_core::{CommandId, SequenceIds};

use crate::AppResult;

/// Platform observations only; game-specific input bindings belong to later phases.
#[derive(Debug, Serialize)]
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
    Wheel {
        x: f64,
        y: f64,
        unit: &'static str,
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
}

#[derive(Serialize)]
struct Record<'a> {
    command_id: CommandId,
    #[serde(flatten)]
    command: &'a InputCommand,
}

/// One JSON object per line, without timestamps or debug-formatted payloads.
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
    writer.flush()?;
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
}
