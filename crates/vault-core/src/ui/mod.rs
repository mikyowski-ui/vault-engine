//! SPEC-014 game flow and user interface rules, independent of windowing:
//! layouts, state machines, timings and text rules that a presentation layer
//! draws and drives. Coordinates are window-relative pixels unless named
//! "screen"; durations are milliseconds.
//!
//! # Modal layout and dispatch (SPEC-020 §2.1)
//!
//! Every modal screen has a deterministic layout query and a dispatch
//! method here, so a presentation layer keeps no button rectangles and no
//! copy of the modal state. The shared pieces:
//!
//! - [`Layout`] lists a screen's controls with their rectangles in
//!   hit-test order; `hit(point)` is the control under a point and
//!   `rect(control)` a control's rectangle (for its down art). Layouts are
//!   pure functions of their inputs and are in *frame* pixels: the 640 by
//!   480 frame ([`FRAME_SIZE`]) every screen draws into. Under the
//!   unlocked widescreen view the frame is only placed on a wider surface:
//!   map a surface point with [`frame_point`] before hit-testing, and use
//!   [`Layout::offset`] to place drawn rectangles. [`Rect::offset`],
//!   [`Rect::at`] and [`red_button`] build rectangles.
//! - [`Armed`] is SPEC-014's press-then-release rule: a press arms the
//!   control under the pointer and the release fires it only over the same
//!   control. Each screen's `press` / `release` methods use it, and its
//!   `pressed()` is the control to draw down.
//! - Each screen returns an *outcome* enum from its input methods. The
//!   outcome names what the caller must do with world, host, audio or
//!   file state the UI does not own (load a slot, start a rest, play a
//!   movie); the screen's own state has already changed. Press and release
//!   sounds stay with the caller: each control type documents its pair
//!   (`sounds()` where they differ by control). An input the screen does
//!   not accept returns the outcome's `None` variant and must still be
//!   swallowed while the screen is modal (§2.1 rule 4).
//!
//! The screens, one paragraph each:
//!
//! **Main menu** ([`flow::MainMenu`]). `MainMenu::new(fallout2)`;
//! `layout()` gives the buttons by index (0-4, or 0-5 under Fallout 2);
//! `poll(MainMenuInput, shift_held, elapsed_ms, opaque)` takes keys and
//! [`Pointer`] presses and releases (the `opaque(index, point)` closure is
//! the up art's nonzero-pixel test) and returns the existing
//! [`flow::MenuPoll`] (choice, sounds, brightness). `pressed()` is the
//! button drawn down. [`flow::menu_layout`] and `MenuButton::at` are the
//! Fallout 1 rectangles alone.
//!
//! **Premade selection** ([`flow::SelectionScreen`]). Holds the shown
//! premade `index`. `press(point)` arms a control (play
//! [`flow::selection_sounds`]); `release(point)` and `key(SelectionKey)`
//! return a [`flow::SelectionOutcome`] (`Take(i)`, `Modify(i)`, `Create`,
//! `Back`, `Moved(i)`, ...); `dispatch(SelectionAction)` acts directly.
//! [`flow::selection_layout`] is the six controls.
//!
//! **Dialog boxes** ([`flow::DialogBox`]). `DialogBox::new(origin,
//! yes_no)` (or `sized`); `layout()` keys the red buttons by answer
//! (`true` Done/Yes, `false` No). The in-game boxes answer on the press:
//! `click(point)`; press-then-release boxes use `press` / `release`;
//! `key(DialogKey)` is `dialog_answer`.
//!
//! **Options menu, pause and quit confirmation**
//! ([`options::OptionsWindow`]). `page` is Menu, Pause, Quit or
//! Preferences. `click(point)` (controls act on the press) and
//! `key(MenuKey)` return an [`options::OptionsOutcome`]: `Save` / `Load`
//! (open the save screen with `from_options`), `Page(..)` (the window now
//! shows the preferences or the quit box), `Close`, or `Quit(answer)`.
//! `layout()` is the page's buttons; [`options::options_layout`] the
//! menu's five on screen.
//!
//! **Preferences** ([`options::PreferencesScreen`]). `open(values)` keeps
//! the backup Cancel restores. Measure the labels once with
//! [`options::LabelWidths::measure`]; `layout(&widths, covered)` (the
//! fifth knob is left out while covered) and `press(point, &widths,
//! covered, now)` / `release(point, &widths, now)` drive the red buttons,
//! knobs, switches, sliders and checkbox. `drag_tick(now, pointer_x)` runs
//! one slider iteration ([`options::DragStep`]); `key(PreferenceKey, now)`
//! handles Esc, Done and the quit keys; `busy(now)` says whether input is
//! swallowed; `close(keep)` returns the values to apply.
//!
//! **Save/load** ([`saveload::SaveLoadScreen`]).
//! `open(mode, slot_states, slot_list)`; `into_list()` keeps the
//! selection for the next opening. `press(point, now)` and
//! `key(SaveLoadKey)` return a [`saveload::SaveLoadOutcome`]
//! (`AskOverwrite`, `Notice`, `Load(slot)`, `Save { slot, description }`,
//! `Cancel { to_options }`, ...). After a Yes to the overwrite box call
//! `overwrite_accepted()`; after a failed save `rescan(slots)`. Each frame
//! call `frame(now, pointer_on_held_arrow)`; on the left release
//! `release()`. [`saveload::layout`] is the controls (the description
//! box's two while it is open); `arrow_held()` is the arrow drawn down.
//!
//! **Pip-Boy** ([`pipboy::PipBoyModal`]). `open(now)`; every call takes
//! the current [`pipboy::PageCounts`] (list lengths). `press(point,
//! counts, now)`, `release(point, counts, now)` and `key(PipBoyKey, now)`
//! return a [`pipboy::PipBoyOutcome`]. Data-bound outcomes come back to
//! the modal: `AlarmRequested` → `show_alarm()` when resting is allowed;
//! `Location(r)` → `flash(now, Highlight::Location(r), Deferred::Detail(key))`;
//! `Disk(r)` → read it, then `flash(now, Highlight::Disk(r),
//! Deferred::Reading { disk, last_page })`; `Rest(option)` →
//! `begin_rest(option)` once the host rest starts, `end_rest()` when it
//! ends. Each pass: `tick(now)` ends a due flash, `rest_due(now)` paces
//! rest steps, `saver_due(now, dialog)` and `saver_pass(now, draw)` run
//! the screen saver; pointer movement calls `input(now)`. `layout(counts)`
//! is the buttons and the page's zones; `page` and `flash` are what to
//! draw. An automap elevation view's `area` is the index into the sorted
//! area list.
//!
//! **Dialogue panel, Review and Ask About** ([`talk::Panel`],
//! [`talk::ReviewWindow`], [`talk::AskAboutWindow`]). The panel's
//! `press` / `release` return the [`talk::PanelControl`] to act on
//! (`panel_key` maps `a`, `b`, `c`); `PanelControl::sounds()` is its cue
//! pair. Review's `press` / `release(point, entries)` / `key(ReviewKey,
//! entries)` return [`talk::ReviewOutcome`] (`Scrolled`, `Close`) and keep
//! `top`. Ask About's `press` / `release` / `key(AskKey, measure, glyph)`
//! return [`talk::AskOutcome`] (`Submit(text)`, `Cancel`); call
//! `answer_shown()` when the lookup answered, `tick(now)` each input poll,
//! and `hides()` for the reply and option windows.
//!
//! **Called shot** ([`called::CalledShotWindow`]). `new(label_widths,
//! cancel_size)` with each location's label width in font 101.
//! `press(point)` arms Cancel or a part; `release(point)` and
//! `key(CalledKey)` return [`called::CalledOutcome`] (`Cancel` or
//! `Choose(part)`). [`called::layout`] is Cancel then the part buttons on
//! screen.
//!
//! **Inventory and use-on** ([`inventory::InventoryWindow`]). Holds the
//! list `offset` and the cursor (`arrow`). `press(point, count,
//! red_size)`, `right_press()` and `key(InventoryKey, count)` return
//! [`inventory::InventoryOutcome`] (`Close`, `Scrolled`, `Entry(index)`,
//! `Slot(..)`, `UpLevel`, ...); the lifted item, the item menu and the
//! move counter stay with the caller. [`inventory::inventory_layout`] is
//! the controls; [`inventory::scroll_list`] and [`inventory::list_step`]
//! are the shared scroll rules.
//!
//! **Loot** ([`inventory::loot_layout`]). Fill a [`inventory::LootView`]
//! (companion, Take All, bodies, pane counts and offsets) and hit-test the
//! [`inventory::LootControl`]s on screen; scroll with `scroll_list` over
//! `crate::skills::LOOT_ROWS`. The `LOOT_*` constants are the window's
//! parts.
//!
//! **Barter** ([`inventory::barter_layout`]). Fill a
//! [`inventory::BarterView`] and hit-test the [`inventory::BarterControl`]s
//! on screen; [`inventory::barter_drop`] is the column a drag drops into
//! and [`inventory::barter_route`] its failure message. The trade rules
//! stay in `crate::barter` (Decision D-2).
//!
//! **Character screen** ([`character::CharacterScreen`]). `new(selected,
//! folder, cursor)` (the three persist between openings).
//! `press(point, tab_mask)`, `release(point)` and `key(CharacterKey)`
//! return [`character::CharacterOutcome`] (`Close { keep }`, `Print`,
//! `Select(line)`, `Folder(f)`, `SkillStep { plus }`, ...). A
//! `SkillStep` from a press that stepped calls `hold_slider(plus, now)`;
//! `tick(now, pointer)` returns a due repeat step, and a refused one calls
//! `stop_slider()`. [`character::character_layout`] is the buttons for
//! the cursor row.
//!
//! **Perk chooser** ([`character::PerkChooser`]). `new(n, now)`;
//! `press(point, now)`, `release(point)` and `key(ChooserKey)` return
//! [`character::ChooserOutcome`] (`Moved`, `Choose`, `Cancel`);
//! `tick(now, pointer)` runs the idle timer and the arrow repeat;
//! `list.selected()` is the chosen entry.
//!
//! **Character editor** ([`editor::Editor`]). [`editor::editor_hit`]
//! (with the top buttons' mask test) or [`editor::editor_layout`] gives
//! the [`editor::EditorControl`]; `Editor::dispatch(control)` performs it
//! and returns [`editor::EditorOutcome`] (`Stepped`, `Toggled`,
//! `Refused`, `Finished(sheet)`, `Cancel`, `Open(dialog)`).
//! [`editor::editor_dialog_layout`] lays out the name, age, sex, options
//! and Erase windows; `step_age` and `set_female` act in them.
//!
//! **Sheet file dialogs** ([`sheet::SheetDialog`]). `new(kind, names,
//! default, now)`; `press(point, now)` returns the held rectangle,
//! `release(point)` and `key(FileKey)` a [`sheet::FileEvent`];
//! `tick(now, pointer)` repeats arrows and runs the idle frames; drain
//! `sounds` after each call. [`sheet::file_layout`] is the controls.
//!
//! **Automap** ([`automap::AutomapWindow`]). `open(scanner)`;
//! `press(point, high_detail)`, `release(point)` and `key(AutomapKey)`
//! return [`automap::AutomapOutcome`] (`Detail(high)`, `Scanner`,
//! `Close`, ...); call `scanner_on()` when the Motion Sensor test passes.
//!
//! **Skilldex** ([`skilldex::SkilldexWindow`]). `new(target)` (the
//! action menu's target, or `None`). `press(point, now)` and
//! `key(SkilldexKey)` return [`skilldex::SkilldexOutcome`] (`Chosen { row,
//! skill }` with [`skilldex::ROW_SOUNDS`], `Cancel`, `Busy` while a row
//! waits, `None` for a key it does not take); each pass `due(now)` gives
//! the [`skilldex::SkilldexAction`] (`Choose(skill)` or `UseOn { target,
//! skill }`) once the row's wait is over. [`skilldex::layout`] is the
//! rows then Cancel on screen.
//!
//! **Elevator** ([`elevator::ElevatorWindow`]). `open(kind, view, start,
//! background_size)`; `press(point)`, `release(point, now)` and
//! `key(ElevatorKey, now)` return [`elevator::ElevatorOutcome`]
//! (`Pressed(level)`, `Travel { sound }`, `Perform(choice)`, `Busy`
//! during the travel animation); each frame `tick(now)` returns `Redraw`
//! or, once the animation ends, `Perform(Some(level))`. `layout()` is the
//! level buttons and `gauge_cell(now)` the gauge cell to draw.
//!
//! **Interface bar** ([`bar::BarButtons`]). `press(point)` holds a button
//! (`BarButton::sounds()`); `release(point)` returns the [`bar::Key`] it
//! sends. Set `combat_buttons` while the combat panel shows End Turn and
//! End Combat; [`bar::monitor_area`] is the message monitor's halves.
//!
//! **Fallout 2 combat control** ([`fallout2::ControlPanelWindow`]).
//! `new(allowed_dispositions, custom_background)`; `press` / `release` /
//! `key(ControlKey)` return [`fallout2::ControlOutcome`] (`Disposition`,
//! `BestWeapon`, `Cycle(field)`, `Custom(open)`, `Talk`, ...).
//! [`fallout2::world_panel_layout`] and [`fallout2::entrance_layout`] are
//! the world map's side panel and the town map's entrances.
//!
//! The cursor's action menu ([`cursor::MenuTracker`]) and the endgame
//! credits ([`endgame::Credits::input`]) have no hit boxes: their state
//! machines above are already the whole dispatch.
pub mod automap;
pub mod bar;
pub mod called;
pub mod character;
pub mod cursor;
pub mod editor;
pub mod elevator;
pub mod endgame;
pub mod fallout2;
pub mod flow;
pub mod inventory;
pub mod options;
pub mod pipboy;
pub mod saveload;
pub mod sheet;
pub mod skilldex;
pub mod talk;

use vault_formats::Palette;

/// One modal UI frame lasts at least `trunc(1000 / 24)` milliseconds.
pub const FRAME_MS: u64 = 1000 / 24;

/// A held button's repeat (SPEC-014 §3.9 SPECIAL plus and minus from the
/// 20th frame, §3.10 the age arrows from the 15th, §3.14 the save/load
/// arrows after 13 idle frames): the press steps once at once; after the
/// idle frames of `FRAME_MS` the steps come at 5 per second, rising by 1
/// per step to 24 per second, each step waiting `trunc(1000 / rate)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldRepeat {
    next: u64,
    steps: u32,
}
impl HoldRepeat {
    /// A button pressed at `pressed_ms`, repeating after `idle_frames`.
    pub fn new(pressed_ms: u64, idle_frames: u32) -> Self {
        Self {
            next: pressed_ms + u64::from(idle_frames) * FRAME_MS,
            steps: 0,
        }
    }
    /// Whether a repeated step is due at `now`; a due step schedules the
    /// next one. At most one step per poll, as one per loop pass.
    pub fn poll(&mut self, now: u64) -> bool {
        if now < self.next {
            return false;
        }
        // The step happens now, then waits for the rate's delay.
        self.next = now + editor::repeat_delay(self.steps);
        self.steps += 1;
        true
    }
}

/// SPEC-014 §3.17 the grey lookup table the pause window converts the view
/// through, built from the `color.pal` palette (not the brightness-adjusted
/// or cycled display): for each entry, s is the largest plus the smallest of
/// its five-bit components (an unmapped entry counts as zero), `v =
/// trunc(s × 240 / 510)` (0-29), and the grey index is the palette's RGB555
/// lookup of (v, v, v), key `1057 v`. Unmapped entries therefore map to the
/// lookup of key 0 (index 228 in the corpus).
pub fn grey_table(palette: &Palette) -> [u8; 256] {
    std::array::from_fn(|index| {
        let index = index as u8;
        let components = if palette.is_mapped(index) {
            palette.rgb6(index).map(|level| u16::from(level / 2))
        } else {
            [0; 3]
        };
        let largest = components.into_iter().max().unwrap_or(0);
        let smallest = components.into_iter().min().unwrap_or(0);
        let v = ((largest + smallest) * 240 / 510) as u8;
        // v is at most 29, always a valid five-bit level.
        palette.lookup_rgb5([v; 3]).unwrap_or(0)
    })
}
/// Red-button press and release sounds.
pub const BUTTON_PRESS: &str = "ib1p1xx1";
pub const BUTTON_RELEASE: &str = "ib1lu1x1";
/// The refusal / invalid-input sound.
pub const ERROR_SOUND: &str = "iisxxxx1";
/// Little red button art and size.
pub const RED_BUTTON: (&str, &str, [i32; 2]) = ("lilredup.frm", "lilreddn.frm", [15, 16]);

/// An RGB555 color key, `1024 r + 32 g + b` with 5-bit channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ColorKey(pub u16);
impl ColorKey {
    pub const LABEL: Self = Self(18979);
    pub const LABEL_PRESSED: Self = Self(14723);
    pub const GREEN: Self = Self(992);
    pub const SELECTED: Self = Self(32747);
    pub const ERROR: Self = Self(32328);
    pub const AMBER: Self = Self(21091);
    pub const GREY_GREEN: Self = Self(21204);
    pub const WHITE: Self = Self(32767);
    pub const GREY: Self = Self(21140);
    pub const DARK_GREEN: Self = Self(1313);
    pub const DARK_OLIVE: Self = Self(15845);
    pub const BLACK: Self = Self(0);
    pub const RED: Self = Self(31744);
    pub fn rgb5(self) -> [u8; 3] {
        let key = self.0;
        [
            (key >> 10 & 31) as u8,
            (key >> 5 & 31) as u8,
            (key & 31) as u8,
        ]
    }
    /// The palette index the color.pal RGB555 table returns for this key.
    pub fn resolve(self, palette: &Palette) -> u8 {
        palette.lookup_rgb5(self.rgb5()).unwrap_or(0)
    }
}

/// A rectangle `(x, y) w by h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, point: [i32; 2]) -> bool {
        (self.x..self.x + self.w).contains(&point[0])
            && (self.y..self.y + self.h).contains(&point[1])
    }
    /// A `size` rectangle with its top-left at `origin`.
    pub const fn at(origin: [i32; 2], size: [i32; 2]) -> Self {
        Self::new(origin[0], origin[1], size[0], size[1])
    }
    /// The same rectangle moved by `by` (a window's origin, or the frame's
    /// placement on a wider surface).
    pub const fn offset(self, by: [i32; 2]) -> Self {
        Self::new(self.x + by[0], self.y + by[1], self.w, self.h)
    }
    /// The top-left corner.
    pub const fn origin(&self) -> [i32; 2] {
        [self.x, self.y]
    }
}

/// The logical frame every modal screen lays out in: 640 by 480 pixels.
/// Under the unlocked widescreen view the frame is placed (and scaled) on
/// a wider surface; its layouts do not change, only the pointer mapping
/// into the frame ([`frame_point`]) and the drawn placement
/// ([`Layout::offset`]).
pub const FRAME_SIZE: [i32; 2] = [640, 480];
/// A surface point in frame pixels, for a frame whose top-left sits at
/// `frame_origin` on the surface (both in frame pixels, after any scale);
/// `None` outside the frame.
pub fn frame_point(point: [i32; 2], frame_origin: [i32; 2]) -> Option<[i32; 2]> {
    let local = [point[0] - frame_origin[0], point[1] - frame_origin[1]];
    Rect::at([0, 0], FRAME_SIZE)
        .contains(local)
        .then_some(local)
}
/// The little red button (`lilredup.frm`, 15 by 16) with its top-left at
/// `at`.
pub const fn red_button(at: [i32; 2]) -> Rect {
    Rect::at(at, RED_BUTTON.2)
}

/// SPEC-020 §2.1 a modal screen's controls with their frame rectangles, in
/// hit-test order: the first control whose rectangle holds a point is the
/// one under it. Layouts are pure functions of their inputs, so the same
/// state always yields the same rectangles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout<C> {
    controls: Vec<(C, Rect)>,
}
impl<C> Default for Layout<C> {
    fn default() -> Self {
        Self {
            controls: Vec::new(),
        }
    }
}
impl<C: Copy + PartialEq> Layout<C> {
    pub fn new() -> Self {
        Self::default()
    }
    /// Append a control (tested after every earlier one).
    pub fn push(&mut self, control: C, rect: Rect) {
        self.controls.push((control, rect));
    }
    /// Builder form of [`Layout::push`].
    pub fn with(mut self, control: C, rect: Rect) -> Self {
        self.push(control, rect);
        self
    }
    /// The control under `point`, if any.
    pub fn hit(&self, point: [i32; 2]) -> Option<C> {
        self.controls
            .iter()
            .find(|(_, rect)| rect.contains(point))
            .map(|(control, _)| *control)
    }
    /// The rectangle of `control` (its first entry), if it is laid out.
    pub fn rect(&self, control: C) -> Option<Rect> {
        self.controls
            .iter()
            .find(|(c, _)| *c == control)
            .map(|(_, rect)| *rect)
    }
    /// Whether `control` is laid out.
    pub fn contains(&self, control: C) -> bool {
        self.rect(control).is_some()
    }
    /// Every control and its rectangle, in hit-test order.
    pub fn controls(&self) -> &[(C, Rect)] {
        &self.controls
    }
    pub fn len(&self) -> usize {
        self.controls.len()
    }
    pub fn is_empty(&self) -> bool {
        self.controls.is_empty()
    }
    /// Every rectangle moved by `by`.
    pub fn offset(self, by: [i32; 2]) -> Self {
        Self {
            controls: self
                .controls
                .into_iter()
                .map(|(control, rect)| (control, rect.offset(by)))
                .collect(),
        }
    }
}
impl<C: Copy + PartialEq> FromIterator<(C, Rect)> for Layout<C> {
    fn from_iter<I: IntoIterator<Item = (C, Rect)>>(iter: I) -> Self {
        Self {
            controls: iter.into_iter().collect(),
        }
    }
}
impl<C: Copy + PartialEq> Extend<(C, Rect)> for Layout<C> {
    fn extend<I: IntoIterator<Item = (C, Rect)>>(&mut self, iter: I) {
        self.controls.extend(iter);
    }
}

/// SPEC-014's press-then-release rule for a control that acts on its
/// release: a press arms the control under the pointer, and the release
/// fires it only over the same control. Any release disarms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Armed<C> {
    pressed: Option<C>,
}
impl<C> Default for Armed<C> {
    fn default() -> Self {
        Self { pressed: None }
    }
}
impl<C: Copy + PartialEq> Armed<C> {
    pub fn new() -> Self {
        Self::default()
    }
    /// A press over `control` (`None`: over nothing) arms it; returns it.
    pub fn press(&mut self, control: Option<C>) -> Option<C> {
        self.pressed = control;
        control
    }
    /// A release over `over`: the armed control when it is the same one.
    pub fn release(&mut self, over: Option<C>) -> Option<C> {
        let pressed = self.pressed.take()?;
        (over == Some(pressed)).then_some(pressed)
    }
    /// The armed control (drawn with its down art).
    pub fn pressed(&self) -> Option<C> {
        self.pressed
    }
    pub fn clear(&mut self) {
        self.pressed = None;
    }
}

/// A pointer input in frame pixels: a left press, or a left release (`None`
/// when it happens outside the frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    Press([i32; 2]),
    Release(Option<[i32; 2]>),
}

/// Font number `100 + n` selects `font<n>.aaf`.
pub const FONT_SMALL: i32 = 101;
pub const FONT_TITLE: i32 = 102;
pub const FONT_CAPTION: i32 = 103;
pub const FONT_LARGE: i32 = 104;
pub const FONT_PLAIN: i32 = 100;

/// Horizontal origin that centers text of `width` in `box_width`.
pub fn centered(box_width: i32, width: i32) -> i32 {
    (box_width - width) / 2
}

#[cfg(test)]
mod tests;
