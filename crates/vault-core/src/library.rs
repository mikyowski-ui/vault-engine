use crate::{
    WorldEffect,
    assets::WorldAssets,
    host::{HostResult, WorldHost},
};
use std::collections::BTreeMap;
use vault_vm::{
    ExternalValue, Value, Vm, VmError,
    extensions::{HostCall, base_tag},
};

#[derive(Debug, Clone)]
pub struct Widget {
    pub rectangle: [i32; 4],
    pub text: Vec<u8>,
    pub flags: i32,
    pub callbacks: [u32; 4],
    pub right: [u32; 2],
    pub program: u32,
    /// Button graphics: pressed, normal, hover; `None` is a null image.
    pub images: [Option<Vec<u8>>; 3],
}
#[derive(Debug, Clone)]
pub struct ManagedWindow {
    pub id: i32,
    pub rectangle: [i32; 4],
    pub cursor: [i32; 2],
    pub fill: [i32; 3],
    pub text: Vec<u8>,
    pub buttons: BTreeMap<Vec<u8>, Widget>,
    pub regions: BTreeMap<Vec<u8>, Widget>,
}
#[derive(Debug, Clone)]
pub struct SoundState {
    pub filename: Vec<u8>,
    pub mode: i32,
    pub streamed: bool,
    pub stereo: bool,
    pub playing: bool,
    pub paused: bool,
}
#[derive(Debug, Clone)]
pub struct NamedBinding {
    pub name: Vec<u8>,
    pub program: u32,
    pub procedure: u32,
    pub persistent: bool,
    pub hits: u32,
    pub busy: bool,
}
#[derive(Debug)]
pub struct LibraryState {
    pub windows: BTreeMap<Vec<u8>, ManagedWindow>,
    pub current: i32,
    pub selection: Vec<i32>,
    pub resolution: [i32; 2],
    next_window: i32,
    pub font: i32,
    pub text_flags: i32,
    pub text_color: [i32; 3],
    pub highlight: [i32; 3],
    pub mouse_hidden: bool,
    pub mouse_shape: Option<(Vec<u8>, [i32; 2])>,
    pub sounds: [Option<SoundState>; 32],
    pub sound_trace: Vec<(u16, u32)>,
    pub named: [Option<NamedBinding>; 40],
    pub keys: BTreeMap<i32, (u32, u32)>,
    pub any_key: Option<(u32, u32)>,
    pub dialogue_depth: i32,
    pub start_position: i32,
    pub reply_title: Option<Vec<u8>>,
    pub reply_flags: u16,
    pub option_flags: u16,
    pub reply_color: [i32; 3],
    pub option_color: [i32; 3],
    pub spacing: i32,
    pub border: [i32; 2],
    pub reply_window: Option<([i32; 4], Option<Vec<u8>>)>,
    pub option_window: Option<([i32; 4], Option<Vec<u8>>)>,
    pub scroll: [Option<Vec<Option<Vec<u8>>>>; 2],
    pub timeout: u32,
    pub media_flags: u32,
    pub faded_out: bool,
    pub movie_playing: bool,
    pub movie_flags: u32,
    pub selected_file: Option<Vec<u8>>,
    pub saved_line: i32,
    pub saved_reply: i32,
    pub current_reply: i32,
    pub palette_cycling: bool,
    pub music_paused: bool,
    /// Palette/color tables installed by `loadpalettetable`, with a counter
    /// that presentation layers compare to notice replacements.
    pub palette: Option<std::sync::Arc<vault_formats::Palette>>,
    pub palette_generation: u32,
}
impl Default for LibraryState {
    fn default() -> Self {
        Self {
            windows: BTreeMap::new(),
            current: -1,
            selection: Vec::new(),
            resolution: [640, 480],
            next_window: 1,
            font: 0,
            text_flags: 0,
            text_color: [31; 3],
            highlight: [31; 3],
            mouse_hidden: false,
            mouse_shape: None,
            sounds: std::array::from_fn(|_| None),
            sound_trace: Vec::new(),
            named: std::array::from_fn(|_| None),
            keys: BTreeMap::new(),
            any_key: None,
            dialogue_depth: -1,
            start_position: 0,
            reply_title: None,
            reply_flags: 0,
            option_flags: 0,
            reply_color: [31; 3],
            option_color: [31; 3],
            spacing: 0,
            border: [0; 2],
            reply_window: None,
            option_window: None,
            scroll: [None, None],
            timeout: 0,
            media_flags: 0,
            faded_out: false,
            movie_playing: false,
            movie_flags: 0,
            selected_file: None,
            saved_line: 0,
            saved_reply: 0,
            current_reply: 0,
            palette_cycling: true,
            palette: None,
            palette_generation: 0,
            music_paused: false,
        }
    }
}
fn error(message: &str) -> VmError {
    VmError::Host(message.into())
}
fn key(name: &[u8]) -> Vec<u8> {
    name.iter().map(u8::to_ascii_lowercase).collect()
}
pub(crate) fn text(vm: &Vm, call: &HostCall, index: usize) -> Result<Vec<u8>, VmError> {
    vm.host_string(call.args[index].value)?
        .map(<[u8]>::to_vec)
        .ok_or_else(|| error("null filename/text in legacy unchecked path"))
}
pub(crate) fn formatted(vm: &Vm, value: Value) -> Result<Option<Vec<u8>>, VmError> {
    Ok(match base_tag(value) {
        Value::STATIC_STRING => Some(
            vm.host_string(value)?
                .ok_or_else(|| error("null string"))?
                .to_vec(),
        ),
        Value::INTEGER => Some((value.payload as i32).to_string().into_bytes()),
        Value::FLOAT => Some(vault_vm::float_text(f32::from_bits(value.payload))),
        _ => None,
    })
}
impl LibraryState {
    fn scaled(&self, value: i32, vertical: bool) -> Result<i32, VmError> {
        let divisor = if vertical { 480 } else { 640 };
        i32::try_from(
            (i64::from(value) * i64::from(self.resolution[usize::from(vertical)]) + divisor - 1)
                / divisor,
        )
        .map_err(|_| VmError::Arithmetic)
    }
    fn window_mut(&mut self, id: i32) -> Result<&mut ManagedWindow, VmError> {
        self.windows
            .values_mut()
            .find(|window| window.id == id)
            .ok_or_else(|| error("managed window not found"))
    }
    pub fn cleanup(&mut self, program: u32) {
        self.keys.retain(|_, (owner, _)| *owner != program);
        for binding in &mut self.named {
            if binding
                .as_ref()
                .is_some_and(|binding| binding.program == program)
            {
                *binding = None;
            }
        }
        // SPEC-005 §3.1 safe rule: the any-key binding is cleared with its
        // owning program (the reference leaves it).
        if self.any_key.is_some_and(|(owner, _)| owner == program) {
            self.any_key = None;
        }
    }
    /// A dispatched persistent handler finished; it can accept hits again.
    pub fn release_named(&mut self, program: u32, procedure: u32) {
        for entry in self.named.iter_mut().flatten() {
            if entry.program == program && entry.procedure == procedure {
                entry.busy = false;
            }
        }
    }
    pub fn callbacks(&mut self) -> Vec<(u32, u32)> {
        let mut callbacks = Vec::new();
        for binding in &mut self.named {
            if let Some(entry) = binding
                && !entry.busy
                && entry.hits > 0
            {
                entry.hits -= 1;
                callbacks.push((entry.program, entry.procedure));
                if entry.persistent {
                    // Busy until the dispatched hit has executed: signals in
                    // the meantime are not queued for this binding.
                    entry.busy = true;
                } else {
                    *binding = None;
                }
            }
        }
        callbacks
    }
}
impl<A: WorldAssets> WorldHost<A> {
    pub(crate) fn library_call(&mut self, vm: &mut Vm, call: &HostCall) -> HostResult {
        let word = call.spec.word;
        let values: Vec<_> = call.args.iter().map(|arg| arg.int()).collect();
        let texts: Vec<_> = call.args.iter().map(|arg| arg.text.clone()).collect();
        let color = || -> Result<[i32; 3], VmError> {
            Ok([
                call.args[2].channel.ok_or(VmError::Arithmetic)?,
                call.args[1].channel.ok_or(VmError::Arithmetic)?,
                call.args[0].channel.ok_or(VmError::Arithmetic)?,
            ]
            .map(|value| ((value * 31.0).trunc() as i32).clamp(0, 31)))
        };
        match word {
            0x804c => {
                self.library.dialogue_depth = -1;
            }
            0x804d => {
                vm.status.0 |= 0x20;
                vm.status.0 &= !0x20;
            }
            // SPEC-005 §3.2 safe rule: the new slot is indexed only after
            // the depth increments, so the initial depth -1 is safe.
            0x804e | 0x804f => {
                self.library.start_position = if word == 0x804e { 0 } else { call.int(0) };
                self.library.dialogue_depth = self.library.dialogue_depth.saturating_add(1);
            }
            0x8050 => self.library.reply_title = call.args[0].text.clone(),
            0x8051 => {
                if self.library.dialogue_depth < 0 || call.text(0).is_some() {
                    return Err(error("library reply not found"));
                }
                self.library.current_reply = 0;
            }
            0x8052 => {} // Defined success stub: no reply is created.
            0x8053 => {
                if base_tag(call.args[0].value) == Value::INTEGER {
                    return Err(error("library option has no current reply"));
                }
            }
            0x8054 => return Err(error("library timed-message backend returns failure")),
            0x8055 | 0x8056 => {
                let config = (
                    [call.int(4), call.int(3), call.int(2), call.int(1)],
                    call.args[0].text.clone(),
                );
                if word == 0x8055 {
                    self.library.reply_window = Some(config);
                } else {
                    self.library.option_window = Some(config);
                }
            }
            0x8057 => self.library.border = [call.int(1), call.int(0)],
            0x8058 | 0x8059 => {
                let mut images = vec![call.args[0].text.clone()];
                for index in 1..4 {
                    let value = call.args[index].value;
                    if base_tag(value) == Value::INTEGER && value.payload != 0 {
                        return Err(VmError::Type(value.tag));
                    }
                    images.push(vm.host_string(value)?.map(<[u8]>::to_vec));
                }
                self.library.scroll[usize::from(word == 0x8059)] = Some(images);
            }
            0x805a => self.library.spacing = call.int(0),
            0x805b => self.library.option_color = color()?,
            0x805c => self.library.reply_color = color()?,
            0x805d => {
                if self.library.dialogue_depth < 0 {
                    return Err(error("library dialogue absent"));
                }
                self.library.current_reply = 0;
            }
            0x805e => {
                return Ok(Some(ExternalValue::Value(Value::integer(
                    self.library
                        .saved_reply
                        .checked_mul(65536)
                        .and_then(|v| v.checked_add(self.library.saved_line))
                        .ok_or(VmError::Arithmetic)?,
                ))));
            }
            // Safe rule: `sayreplyflags` stores reply flags (the reference
            // changes the option flags).
            0x805f => self.library.reply_flags = call.uint(0) as u16,
            0x8060 => self.library.option_flags = call.uint(0) as u16,
            0x8061 => {
                if base_tag(call.args[0].value) == 0x4000 {
                    return Err(VmError::Type(call.args[0].value.tag));
                }
                self.library.timeout = call.uint(0);
            }
            0x8062 | 0x8065 | 0x8066 => {
                let name = key(&text(vm, call, 4)?);
                let rectangle = [
                    self.library.scaled(call.int(3), false)?,
                    self.library.scaled(call.int(2), true)?,
                    self.library.scaled(call.int(1), false)?,
                    self.library.scaled(call.int(0), true)?,
                ];
                if rectangle[2] <= 0
                    || rectangle[3] <= 0
                    || rectangle[2] > 8192
                    || rectangle[3] > 8192
                {
                    return Err(error("invalid managed window dimensions"));
                }
                if word == 0x8062 {
                    if self.library.windows.len() >= 64 || self.library.windows.contains_key(&name)
                    {
                        return Err(error("managed window creation failed"));
                    }
                    let id = self.library.next_window;
                    self.library.next_window += 1;
                    self.library.windows.insert(
                        name,
                        ManagedWindow {
                            id,
                            rectangle,
                            cursor: [0; 2],
                            fill: [0; 3],
                            text: Vec::new(),
                            buttons: BTreeMap::new(),
                            regions: BTreeMap::new(),
                        },
                    );
                } else {
                    self.library
                        .windows
                        .get_mut(&name)
                        .ok_or_else(|| error("window not found"))?
                        .rectangle = rectangle;
                }
            }
            0x8063 => {
                let window = self
                    .library
                    .windows
                    .remove(&key(&text(vm, call, 0)?))
                    .ok_or_else(|| error("window not found"))?;
                self.library.selection.retain(|id| *id != window.id);
                self.library.current = self.library.selection.pop().unwrap_or(-1);
                if vm.window == window.id {
                    vm.window = self.library.current;
                }
            }
            0x8064 => {
                let id = self
                    .library
                    .windows
                    .get(&key(call.text(0).unwrap_or(b"")))
                    .ok_or_else(|| error("window not found"))?
                    .id;
                if self.library.selection.len() == 64 {
                    return Err(error("window-selection stack limit"));
                }
                self.library.selection.push(self.library.current);
                self.library.current = id;
                vm.window = id;
            }
            0x8067 => {
                self.library.window_mut(vm.window)?;
                self.library.current = vm.window;
            }
            0x8068 => {
                let fill = color()?;
                self.library.window_mut(vm.window)?.fill = fill;
                self.library.current = vm.window;
            }
            0x8069 => {
                color()?;
                self.library.window_mut(vm.window)?;
                self.library.current = vm.window;
            }
            0x806a | 0x806b | 0x806d => {
                self.library.window_mut(vm.window)?;
                self.library.current = vm.window;
                if !self.assets.available(call.text(0).unwrap_or(b"")) {
                    // SPEC-005: `fillwin3x3` makes a load failure a VM
                    // error; `display`/`displayraw` push nothing and name no
                    // error, so a missing resource there is diagnosed only.
                    if word == 0x806a {
                        return Err(error("managed image resource missing"));
                    }
                    self.world.diagnostic(format!(
                        "{word:#06x}: image resource {} missing",
                        String::from_utf8_lossy(call.text(0).unwrap_or(b""))
                    ));
                }
            }
            0x806c => {
                text(vm, call, 4)?;
                self.library.window_mut(self.library.current)?;
            }
            0x806e => {
                let value = call.args[0].value;
                if base_tag(value) == Value::INTEGER && value.payload != u32::MAX {
                    return Err(VmError::Type(value.tag));
                }
                // Accepted non-S operands resolve to null text: no default path.
                let name = text(vm, call, 0)?;
                let palette = self
                    .assets
                    .palette(&name)
                    .map_err(|failure| error(&format!("palette load failed: {failure}")))?;
                self.library.palette = Some(std::sync::Arc::new(palette));
                self.library.palette_generation = self.library.palette_generation.wrapping_add(1);
            }
            0x806f | 0x8070 => {
                self.library.faded_out = word == 0x8070;
                self.world.emit(WorldEffect::LibraryFade {
                    out: word == 0x8070,
                    duration: call.int(0),
                    from_black: word == 0x806f,
                });
            }
            0x8071 => self.library.window_mut(vm.window)?.cursor = [call.int(1), call.int(0)],
            0x8072 => {
                if let Some(value) = formatted(vm, call.args[0].value)? {
                    self.library.window_mut(vm.window)?.text.extend(value);
                }
            }
            0x8073 => {
                self.library
                    .window_mut(vm.window)?
                    .text
                    .extend(call.text(5).unwrap_or(b""));
            }
            0x8074 => {
                if call.int(0) > 2 {
                    return Err(error("invalid printrect alignment"));
                }
                // Safe rule: an unsupported tag prints nothing, and the text
                // is truncated to the 79 bytes the 80-byte area holds.
                let Some(mut value) = formatted(vm, call.args[2].value)? else {
                    return Ok(None);
                };
                value.truncate(79);
                self.library.window_mut(vm.window)?.text.extend(value);
            }
            0x8075 => {
                // SPEC-002 §3.1 safe rule: selecting an unloaded font is a
                // no-op that keeps the current number, bitmap, and metrics.
                let number = call.int(0);
                if self.assets.font_number(number).is_some() {
                    self.library.font = number;
                } else {
                    self.world
                        .diagnostic(format!("font {number} is not loaded; font unchanged"));
                }
            }
            0x8076 => self.library.text_flags = call.int(0),
            0x8077 => self.library.text_color = color()?,
            0x8078 => self.library.highlight = color()?,
            0x8079 => self.library.movie_playing = false,
            0x807a | 0x807c => {
                let mut name = if word == 0x807a {
                    call.text(0).unwrap_or(b"").to_vec()
                } else {
                    text(vm, call, 4)?
                };
                if !name.contains(&b'.') {
                    name.extend_from_slice(b".mve");
                }
                if !self.assets.available(&name) {
                    return Err(error("movie playback failed"));
                }
                // Headless, playback ends as it starts: the wait predicate
                // below is already satisfied.
                self.library.movie_playing =
                    self.movie_policy == crate::movie::MoviePolicy::WaitUntilFinished;
                self.world
                    .emit(WorldEffect::Movie(crate::movie::MovieRequest {
                        index: None,
                        flags: 0,
                        path: String::from_utf8_lossy(&name).into_owned(),
                        subtitles: None,
                        fades: None,
                    }));
                vm.wait_predicate = 2;
                vm.status.0 |= 0x10;
            }
            0x807b => self.library.movie_flags = call.uint(0),
            0x807f => {
                // SPEC-005 §3.1 safe rule: a name operand that is not S is a
                // VM type error.
                if let Some(arg) = call.args.last()
                    && base_tag(arg.value) != Value::STATIC_STRING
                {
                    return Err(VmError::Type(arg.value.tag));
                }
                let name = call
                    .args
                    .last()
                    .and_then(|arg| arg.text.as_deref())
                    .ok_or_else(|| error("unsafe null region name"))?;
                let mut points = Vec::new();
                for pair in call.args[1..call.args.len() - 1].as_chunks::<2>().0 {
                    points.push(self.library.scaled(pair[1].int(), false)?);
                    points.push(self.library.scaled(pair[0].int(), true)?);
                }
                let window = self.library.window_mut(vm.window)?;
                window.regions.insert(
                    key(name),
                    Widget {
                        rectangle: [0; 4],
                        text: Vec::new(),
                        flags: 0,
                        callbacks: [0; 4],
                        right: [0; 2],
                        program: vm.id,
                        images: Default::default(),
                    },
                );
                self.world.emit(WorldEffect::Gui {
                    opcode: word,
                    values: points,
                    texts: vec![Some(name.to_vec())],
                });
            }
            0x8080..=0x8082 => {
                let name = call.text(call.args.len() - 1).unwrap_or(b"");
                let region = self
                    .library
                    .window_mut(vm.window)?
                    .regions
                    .get_mut(&key(name))
                    .ok_or_else(|| error("region not found"))?;
                region.program = vm.id;
                if word == 0x8080 {
                    region.flags = call.int(0);
                } else if word == 0x8081 {
                    region.callbacks = [call.uint(3), call.uint(2), call.uint(1), call.uint(0)];
                } else {
                    region.right = [call.uint(1), call.uint(0)];
                }
            }
            0x8083 | 0x808c => {
                let value = call.args[0].value;
                if base_tag(value) == Value::INTEGER && value.payload != u32::MAX {
                    return Err(VmError::Type(value.tag));
                }
                let name = if value.payload == u32::MAX {
                    None
                } else {
                    Some(key(&text(vm, call, 0)?))
                };
                let window = self.library.window_mut(vm.window)?;
                let table = if word == 0x8083 {
                    &mut window.regions
                } else {
                    &mut window.buttons
                };
                if let Some(name) = name {
                    if table.remove(&name).is_none() {
                        return Err(error("widget deletion failed"));
                    }
                } else {
                    table.clear();
                }
            }
            0x8084 => {
                let name = key(&text(vm, call, 1)?);
                let region = self
                    .library
                    .window_mut(self.library.current)?
                    .regions
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| error("region not found"))?;
                if let Some(procedure) = usize::try_from(call.int(0))
                    .ok()
                    .and_then(|event| region.callbacks.get(event))
                    .copied()
                {
                    self.callbacks.push_back((region.program, procedure));
                }
            }
            0x8085 => {
                let exists = self
                    .library
                    .window_mut(self.library.current)?
                    .regions
                    .contains_key(&key(call.text(0).unwrap_or(b"")));
                return Ok(Some(ExternalValue::Value(Value::integer(i32::from(
                    exists,
                )))));
            }
            0x8086 => {
                let rectangle = [
                    self.library.scaled(call.int(3), false)?,
                    self.library.scaled(call.int(2), true)?,
                    self.library.scaled(call.int(1), false)?,
                    self.library.scaled(call.int(0), true)?,
                ];
                self.library.window_mut(vm.window)?.buttons.insert(
                    key(call.text(4).unwrap_or(b"")),
                    Widget {
                        rectangle,
                        text: Vec::new(),
                        flags: 0,
                        callbacks: [0; 4],
                        right: [0; 2],
                        program: vm.id,
                        images: Default::default(),
                    },
                );
            }
            // `addbuttonrightproc` (0x808B) attaches to the named button under
            // the SPEC-005 §3.1 safe rule (the reference uses a region).
            0x8087..=0x808b => {
                let last = call.args.len() - 1;
                let name = key(call.text(last).unwrap_or(b""));
                let mut images: [Option<Vec<u8>>; 3] = Default::default();
                if word == 0x8089 {
                    if !call.args[..3].iter().any(|a| {
                        base_tag(a.value) == Value::STATIC_STRING
                            || (base_tag(a.value) == Value::INTEGER && a.uint() == 0)
                    }) {
                        return Err(error("no valid button image operand"));
                    }
                    // Pop order is hover, normal, pressed. Safe rule: each
                    // operand that is not S is no image.
                    for (slot, index) in [(2, 0), (1, 1), (0, 2)] {
                        let value = call.args[index].value;
                        if base_tag(value) == Value::STATIC_STRING {
                            images[slot] = vm.host_string(value)?.map(<[u8]>::to_vec);
                        }
                    }
                }
                let button = self
                    .library
                    .window_mut(vm.window)?
                    .buttons
                    .get_mut(&name)
                    .ok_or_else(|| error("button not found"))?;
                button.program = vm.id;
                match word {
                    0x8087 => button.text = call.text(0).unwrap_or(b"").to_vec(),
                    0x8088 => button.flags = call.int(0),
                    0x8089 => button.images = images,
                    0x808a => {
                        button.callbacks = [call.uint(3), call.uint(2), call.uint(1), call.uint(0)]
                    }
                    0x808b => button.right = [call.uint(1), call.uint(0)],
                    _ => {}
                }
            }
            0x808d => self.library.mouse_hidden = true,
            0x808e => self.library.mouse_hidden = false,
            0x808f => {
                self.library.mouse_shape = Some((
                    call.text(2).unwrap_or(b"").to_vec(),
                    [call.int(1), call.int(0)],
                ))
            }
            0x8090 => self.callbacks.push_back((vm.id, call.uint(0))), // Initial region hit checker is false.
            // Named-event names are truncated to 31 characters (safe rule).
            0x8092 | 0x8093 => {
                let mut name = key(call.text(1).unwrap_or(b""));
                name.truncate(31);
                let slot = self
                    .library
                    .named
                    .iter()
                    .position(|binding| binding.as_ref().is_some_and(|b| b.name == name))
                    .or_else(|| self.library.named.iter().position(Option::is_none));
                if let Some(slot) = slot {
                    self.library.named[slot] = Some(NamedBinding {
                        name,
                        program: vm.id,
                        procedure: call.uint(0),
                        persistent: word == 0x8093,
                        hits: 0,
                        busy: false,
                    });
                }
            }
            0x8094 | 0x8095 => {
                let mut name = key(call.text(0).unwrap_or(b""));
                name.truncate(31);
                if let Some(slot) = self
                    .library
                    .named
                    .iter()
                    .position(|binding| binding.as_ref().is_some_and(|b| b.name == name))
                {
                    if word == 0x8094 {
                        self.library.named[slot] = None;
                    } else if let Some(binding) = &mut self.library.named[slot]
                        && !binding.busy
                    {
                        binding.hits = binding.hits.saturating_add(1);
                    }
                }
            }
            0x8096 | 0x8097 => {
                let key = call.int(if word == 0x8096 { 1 } else { 0 });
                if !(-1..=255).contains(&key) {
                    return Err(error("unsafe key index"));
                }
                if word == 0x8096 {
                    let binding = (vm.id, call.uint(0));
                    if key == -1 {
                        self.library.any_key = Some(binding);
                    } else {
                        self.library.keys.insert(key, binding);
                    }
                } else if key == -1 {
                    self.library.any_key = None;
                } else {
                    self.library.keys.remove(&key);
                }
            }
            0x8098 => {
                let name = call.text(1).unwrap_or(b"").to_vec();
                let mode = call.int(0);
                let slot = self
                    .library
                    .sounds
                    .iter()
                    .position(Option::is_none)
                    .filter(|_| self.assets.available(&name));
                let handle = if let Some(slot) = slot {
                    self.library.sounds[slot] = Some(SoundState {
                        filename: name.clone(),
                        mode,
                        streamed: mode & 512 != 0,
                        stereo: mode & 8192 != 0,
                        playing: true,
                        paused: false,
                    });
                    self.world.emit(WorldEffect::LibrarySound {
                        slot,
                        filename: name,
                        mode,
                    });
                    0xa0000000 | slot as u32
                } else {
                    u32::MAX
                };
                self.library.sound_trace.push((word, handle));
                return Ok(Some(ExternalValue::Value(Value::integer(handle as i32))));
            }
            0x8099..=0x809d => {
                let handle = call.uint(0);
                self.library.sound_trace.push((word, handle));
                // Safe rule: a handle whose stripped slot is outside 0-31 (or
                // empty) is a no-op, like -1.
                let slot = (handle & !0xa000_0000) as usize;
                if handle != u32::MAX && slot < 32 {
                    if word == 0x809d {
                        self.library.sounds[slot] = None;
                    } else if let Some(sound) = &mut self.library.sounds[slot] {
                        match word {
                            0x8099 | 0x809b => {
                                if sound.streamed {
                                    sound.paused = true;
                                } else {
                                    sound.playing = false;
                                }
                            }
                            0x809a => {
                                sound.paused = false;
                                sound.playing = true;
                            }
                            0x809c if sound.playing && !sound.paused => {
                                self.world.emit(WorldEffect::LibrarySound {
                                    slot,
                                    filename: sound.filename.clone(),
                                    mode: sound.mode,
                                })
                            }
                            _ => {}
                        }
                    }
                }
            }
            0x809e => {
                if call.int(0) != 0 {
                    self.library.media_flags &= !8;
                } else {
                    self.library.media_flags |= 8;
                }
            }
            0x809f => {
                vm.status.0 |= 0x20;
                let candidates = self.assets.files(call.text(0).unwrap_or(b""));
                let selected = self
                    .library
                    .selected_file
                    .take()
                    .filter(|selection| candidates.contains(selection));
                vm.status.0 &= !0x20;
                return Ok(Some(selected.map_or(
                    ExternalValue::Value(Value::integer(0)),
                    ExternalValue::String,
                )));
            }
            // Safe rules: an absent previous match returns I zero, and no
            // scan passes the source terminator (so delimiter 0 is safe).
            0x80a0 => {
                let delimiter = call.int(0);
                if !(0..=255).contains(&delimiter) {
                    return Err(error("unsafe tokenizer delimiter"));
                }
                let source = call.text(2).unwrap_or(b"");
                let delimiter = delimiter as u8;
                let token = if let Some(previous) = call.text(1) {
                    let found = if previous.is_empty() {
                        Some(0)
                    } else {
                        source
                            .windows(previous.len())
                            .position(|part| part == previous)
                    };
                    let Some(found) = found else {
                        return Ok(Some(ExternalValue::Value(Value::integer(0))));
                    };
                    let start = found + previous.len();
                    let Some(at) = source[start..]
                        .iter()
                        .position(|byte| *byte == delimiter)
                        .map(|offset| start + offset)
                    else {
                        return Ok(Some(ExternalValue::Value(Value::integer(0))));
                    };
                    let length = source[at + 1..]
                        .iter()
                        .position(|byte| *byte == delimiter)
                        .unwrap_or(source.len() - at - 1);
                    source[at..at + length].to_vec()
                } else {
                    source[..source
                        .iter()
                        .position(|byte| *byte == delimiter)
                        .unwrap_or(source.len())]
                        .to_vec()
                };
                return Ok(Some(ExternalValue::String(token)));
            }
            _ => return Err(VmError::Opcode(word)),
        }
        self.world.emit(WorldEffect::Gui {
            opcode: word,
            values,
            texts,
        });
        Ok(None)
    }
}
