//! The touch UI's gestures (`--touch-ui`, `touch_ui = true`): a headless
//! state machine from touch phases, finger ids and surface positions, read
//! against the shell clock, to the pointer, button and wheel commands the
//! mouse already logs. Engine infrastructure, not part of any
//! specification.
//!
//! Every gesture becomes existing input records (`pointer_moved`,
//! `pointer_left`, `pointer_button` and `wheel`), which the shell logs and
//! applies through the mouse's own handlers, so `--record-inputs` and
//! `--replay-inputs` see a touch session as a mouse session and nothing new
//! enters the log.
//!
//! - **Tap** (one finger, lifted within [`HOLD_PRESS_MS`] and never past the
//!   slop): the pointer moves to the finger when it lands, and a left press
//!   and release follow when it lifts.
//! - **Drag** (one finger past the slop): the pointer follows the finger.
//!   With the camera available (the unlocked view, the touch UI's
//!   navigation-only view under Vanilla included) the drag is a
//!   middle-button drag from where the finger landed, the camera's own pan.
//!   Elsewhere it only moves the pointer.
//! - **Hold** (one finger still for [`HOLD_PRESS_MS`]): the left button
//!   goes down where it rests and stays down, the mouse's press and hold:
//!   the game's own held-button pulse (SPEC-014 §3.28, `MENU_HOLD_MS`
//!   later) opens the action menu over a critter, door or item at
//!   [`LONG_PRESS_MS`], or a window's item menu. The pointer then follows
//!   the finger, so sliding moves the highlight, and the lift releases the
//!   button, picking the entry. Lifted before a menu opens, the hold is a
//!   click. While no menu is open, sliding past the slop or a second
//!   finger withdraws the press (a `pointer_left` before the release, so
//!   it does not act) and the touch drags, or becomes a two-finger
//!   gesture, instead; with a menu open neither interrupts it.
//! - **Two-finger tap** (a second finger lands and the first of the two
//!   lifts within [`LONG_PRESS_MS`] of it, neither past the slop): a right
//!   press and release at the pointer, the cursor-mode toggle (as is the
//!   pill bar's `MODE`).
//! - **Pinch** (two fingers whose distance changes past the slop): with the
//!   camera available, the pointer moves to their midpoint and each change
//!   becomes wheel lines `ln(d / d_last) / ln(1.15)`, the camera's zoom step,
//!   so the zoom follows the fingers' spread about the midpoint and the
//!   camera clamps it to 0.5-3.0. Without the camera (the fixed 640x480
//!   view) nothing happens.
//! - **Button** (one finger landing on an on-screen button, see
//!   [`TouchContext::button`]): the left press is sent at once, so the
//!   button shows pressed, and the release when the finger lifts.
//! - **Cancel** (the system took the touch): a drag ends its middle button;
//!   a held button or hold gets its release with the pointer outside the
//!   window (a `pointer_left` first), so it does not act (an open action
//!   menu still takes its highlighted entry, as a mouse release would);
//!   nothing else is sent.
//!
//! A gesture that has ended or been cancelled ignores every finger until
//! all of them have lifted. A third finger is ignored.
use crate::input::{InputCommand, WheelUnit};
use vault_core::ui::cursor::MENU_HOLD_MS;
use winit::event::TouchPhase;

/// How far a finger may move, in logical pixels, and still tap or press.
pub const SLOP: f64 = 10.0;
/// A finger held this long within the slop opens the action menu (and a
/// TURN button held this long sends Enter).
pub const LONG_PRESS_MS: u64 = 500;
/// A finger still this long gets its left press, so the game's held-button
/// pulse, [`MENU_HOLD_MS`] later, opens the menu at [`LONG_PRESS_MS`].
pub const HOLD_PRESS_MS: u64 = LONG_PRESS_MS - MENU_HOLD_MS;
/// The camera's zoom step per wheel line (`map_preview::camera::ZOOM_STEP`).
const ZOOM_STEP: f64 = 1.15;

/// A pointer button a gesture presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchButton {
    Left,
    Right,
    Middle,
}
impl TouchButton {
    /// The button's name as the input log writes it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Middle => "Middle",
        }
    }
}

/// What a gesture asks of the shell, in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TouchAction {
    /// The pointer moves to a surface point.
    Pointer([f64; 2]),
    /// The pointer leaves the window.
    Leave,
    /// A button goes down or up at the pointer.
    Button { button: TouchButton, pressed: bool },
    /// Wheel lines at the pointer: a pinch's zoom.
    Zoom { lines: f64 },
}
impl TouchAction {
    /// The input records the action is, as the mouse logs them.
    pub fn command(self) -> InputCommand {
        match self {
            Self::Pointer([x, y]) => InputCommand::PointerMoved { x, y },
            Self::Leave => InputCommand::PointerLeft,
            Self::Button { button, pressed } => InputCommand::PointerButton {
                button: button.name().to_owned(),
                pressed,
            },
            Self::Zoom { lines } => InputCommand::Wheel {
                x: 0.0,
                y: lines,
                unit: WheelUnit::Lines,
            },
        }
    }
}

/// What the shell knows about a touch's place when it reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TouchContext {
    /// The camera pans and zooms (the enhanced unlocked view).
    pub camera: bool,
    /// The touch landed on an on-screen button (the touch UI's pill bar):
    /// it presses at once. Read for a first finger landing only.
    pub button: bool,
    /// A held-button menu is open (the map's action menu, or a window's
    /// item menu): a hold slides through it, and neither a slide past the
    /// slop nor a second finger withdraws its press.
    pub menu: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    /// No finger down.
    Idle,
    /// One finger down within the slop: a tap or a long press so far.
    Pending {
        id: u64,
        start: [f64; 2],
        since: u64,
    },
    /// One finger on an on-screen button, its left press sent.
    Holding { id: u64 },
    /// One finger held still past [`HOLD_PRESS_MS`]: its left press sent
    /// where it landed, the pointer following it.
    Pressed { id: u64, start: [f64; 2] },
    /// One finger dragging; `panning` when it holds the middle button.
    Dragging { id: u64, panning: bool },
    /// Two fingers: a tap so far, or a pinch.
    Two {
        ids: [u64; 2],
        starts: [[f64; 2]; 2],
        since: u64,
        /// The fingers' distance when the second landed, and at the last
        /// zoom sent.
        start_distance: f64,
        last_distance: f64,
        pinching: bool,
        moved: bool,
    },
    /// The gesture is over: nothing more until every finger lifts.
    Done,
}

/// The touch gesture state machine.
#[derive(Debug, Clone)]
pub struct TouchGestures {
    /// The slop in surface pixels (the shell scales [`SLOP`] by the
    /// window's scale factor).
    pub slop: f64,
    /// The fingers down, in landing order, at their last positions.
    touches: Vec<(u64, [f64; 2])>,
    state: State,
}
impl Default for TouchGestures {
    fn default() -> Self {
        Self::new(SLOP)
    }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}
fn click(button: TouchButton) -> [TouchAction; 2] {
    [true, false].map(|pressed| TouchAction::Button { button, pressed })
}
/// A hold's left press withdrawn: released with the pointer outside the
/// window, so it does not act.
fn withdraw() -> [TouchAction; 2] {
    [
        TouchAction::Leave,
        TouchAction::Button {
            button: TouchButton::Left,
            pressed: false,
        },
    ]
}

impl TouchGestures {
    pub fn new(slop: f64) -> Self {
        Self {
            slop,
            touches: Vec::new(),
            state: State::Idle,
        }
    }
    /// A finger is down: mouse events are not touch's to mix with.
    pub fn active(&self) -> bool {
        !self.touches.is_empty()
    }
    fn position(&self, id: u64) -> Option<[f64; 2]> {
        self.touches
            .iter()
            .find(|(touch, _)| *touch == id)
            .map(|(_, at)| *at)
    }
    /// One touch event at shell time `now`.
    pub fn feed(
        &mut self,
        phase: TouchPhase,
        id: u64,
        at: [f64; 2],
        now: u64,
        context: TouchContext,
    ) -> Vec<TouchAction> {
        match phase {
            TouchPhase::Started => self.started(id, at, now, context),
            TouchPhase::Moved => self.moved(id, at, context),
            TouchPhase::Ended => self.lifted(id, at, now, false),
            TouchPhase::Cancelled => self.lifted(id, at, now, true),
        }
    }
    /// The frame at shell time `now`: a hold's press comes due.
    pub fn poll(&mut self, now: u64) -> Vec<TouchAction> {
        match self.state {
            State::Pending { id, start, since } if now.saturating_sub(since) >= HOLD_PRESS_MS => {
                self.state = State::Pressed { id, start };
                vec![TouchAction::Button {
                    button: TouchButton::Left,
                    pressed: true,
                }]
            }
            _ => Vec::new(),
        }
    }
    fn started(
        &mut self,
        id: u64,
        at: [f64; 2],
        now: u64,
        context: TouchContext,
    ) -> Vec<TouchAction> {
        if self.position(id).is_some() {
            // A repeated start is a move.
            return self.moved(id, at, context);
        }
        self.touches.push((id, at));
        let mut out = Vec::new();
        self.state = match self.state {
            State::Idle if context.button => {
                out.push(TouchAction::Pointer(at));
                out.push(TouchAction::Button {
                    button: TouchButton::Left,
                    pressed: true,
                });
                State::Holding { id }
            }
            State::Idle => {
                out.push(TouchAction::Pointer(at));
                State::Pending {
                    id,
                    start: at,
                    since: now,
                }
            }
            State::Pending {
                id: first, start, ..
            } => self.two(first, start, id, at, now),
            // A hold with no menu open yet gives way to the two fingers,
            // the pointer back where the first rests.
            State::Pressed { id: first, .. } if !context.menu => {
                out.extend(withdraw());
                let start = self.position(first).unwrap_or(at);
                out.push(TouchAction::Pointer(start));
                self.two(first, start, id, at, now)
            }
            State::Dragging { id: first, panning } => {
                if panning {
                    out.push(TouchAction::Button {
                        button: TouchButton::Middle,
                        pressed: false,
                    });
                }
                let start = self.position(first).unwrap_or(at);
                let mut two = self.two(first, start, id, at, now);
                if let State::Two { moved, .. } = &mut two {
                    // A drag that becomes two fingers is no tap.
                    *moved = true;
                }
                two
            }
            // A finger beside a held button, or a third, is ignored.
            other => other,
        };
        out
    }
    fn two(&self, first: u64, start: [f64; 2], id: u64, at: [f64; 2], now: u64) -> State {
        let spread = distance(start, at).max(1.0);
        State::Two {
            ids: [first, id],
            starts: [start, at],
            since: now,
            start_distance: spread,
            last_distance: spread,
            pinching: false,
            moved: false,
        }
    }
    fn moved(&mut self, id: u64, at: [f64; 2], context: TouchContext) -> Vec<TouchAction> {
        let Some(slot) = self.touches.iter_mut().find(|(touch, _)| *touch == id) else {
            return Vec::new();
        };
        slot.1 = at;
        let slop = self.slop;
        match &mut self.state {
            State::Pending {
                id: finger, start, ..
            } if *finger == id => {
                if distance(*start, at) <= slop {
                    return Vec::new();
                }
                let panning = context.camera;
                self.state = State::Dragging { id, panning };
                if panning {
                    vec![
                        TouchAction::Button {
                            button: TouchButton::Middle,
                            pressed: true,
                        },
                        TouchAction::Pointer(at),
                    ]
                } else {
                    vec![TouchAction::Pointer(at)]
                }
            }
            State::Pressed { id: finger, start } if *finger == id => {
                if distance(*start, at) <= slop || context.menu {
                    return vec![TouchAction::Pointer(at)];
                }
                // No menu opened: the press is withdrawn and the finger
                // drags from where it landed.
                let (start, panning) = (*start, context.camera);
                self.state = State::Dragging { id, panning };
                let mut out = withdraw().to_vec();
                if panning {
                    out.extend([
                        TouchAction::Pointer(start),
                        TouchAction::Button {
                            button: TouchButton::Middle,
                            pressed: true,
                        },
                    ]);
                }
                out.push(TouchAction::Pointer(at));
                out
            }
            State::Dragging { id: finger, .. } | State::Holding { id: finger } if *finger == id => {
                vec![TouchAction::Pointer(at)]
            }
            State::Two {
                ids,
                starts,
                start_distance,
                last_distance,
                pinching,
                moved,
                ..
            } if ids.contains(&id) => {
                let [a, b] = *ids;
                let (Some(pa), Some(pb)) = (
                    self.touches.iter().find(|t| t.0 == a).map(|t| t.1),
                    self.touches.iter().find(|t| t.0 == b).map(|t| t.1),
                ) else {
                    return Vec::new();
                };
                if distance(starts[0], pa) > slop || distance(starts[1], pb) > slop {
                    *moved = true;
                }
                let spread = distance(pa, pb).max(1.0);
                if !*pinching && (spread - *start_distance).abs() > slop {
                    *pinching = true;
                }
                if !*pinching || !context.camera {
                    return Vec::new();
                }
                let lines = (spread / *last_distance).ln() / ZOOM_STEP.ln();
                *last_distance = spread;
                if lines == 0.0 {
                    return Vec::new();
                }
                vec![
                    TouchAction::Pointer(midpoint(pa, pb)),
                    TouchAction::Zoom { lines },
                ]
            }
            _ => Vec::new(),
        }
    }
    fn lifted(&mut self, id: u64, at: [f64; 2], now: u64, cancelled: bool) -> Vec<TouchAction> {
        let Some(index) = self.touches.iter().position(|(touch, _)| *touch == id) else {
            return Vec::new();
        };
        self.touches.remove(index);
        let mut out = Vec::new();
        match self.state {
            // A tap, or a hold the frames did not see in time: a click.
            State::Pending { id: finger, .. } if finger == id => {
                if !cancelled {
                    out.extend(click(TouchButton::Left));
                }
                self.state = State::Done;
            }
            State::Holding { id: finger } | State::Pressed { id: finger, .. } if finger == id => {
                if cancelled {
                    out.push(TouchAction::Leave);
                } else {
                    out.push(TouchAction::Pointer(at));
                }
                out.push(TouchAction::Button {
                    button: TouchButton::Left,
                    pressed: false,
                });
                self.state = State::Done;
            }
            State::Dragging {
                id: finger,
                panning,
            } if finger == id => {
                if panning {
                    out.push(TouchAction::Button {
                        button: TouchButton::Middle,
                        pressed: false,
                    });
                }
                self.state = State::Done;
            }
            State::Two {
                ids,
                since,
                pinching,
                moved,
                ..
            } if ids.contains(&id) => {
                if !cancelled && !pinching && !moved && now.saturating_sub(since) < LONG_PRESS_MS {
                    out.extend(click(TouchButton::Right));
                }
                self.state = State::Done;
            }
            _ => {}
        }
        if self.touches.is_empty() {
            self.state = State::Idle;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TouchPhase::{Cancelled, Ended, Moved, Started};

    const MAP: TouchContext = TouchContext {
        camera: false,
        button: false,
        menu: false,
    };
    const CAMERA: TouchContext = TouchContext {
        camera: true,
        button: false,
        menu: false,
    };
    const BUTTON: TouchContext = TouchContext {
        camera: true,
        button: true,
        menu: false,
    };
    /// The camera view with a held-button menu open.
    const MENU: TouchContext = TouchContext {
        camera: true,
        button: false,
        menu: true,
    };
    fn press(button: TouchButton) -> TouchAction {
        TouchAction::Button {
            button,
            pressed: true,
        }
    }
    fn release(button: TouchButton) -> TouchAction {
        TouchAction::Button {
            button,
            pressed: false,
        }
    }
    use TouchButton::{Left, Middle, Right};

    #[test]
    fn a_tap_moves_the_pointer_then_left_clicks_where_it_landed() {
        let mut touch = TouchGestures::default();
        assert_eq!(
            touch.feed(Started, 7, [100.0, 50.0], 1000, MAP),
            [TouchAction::Pointer([100.0, 50.0])]
        );
        assert!(touch.active());
        // A wobble within the slop is no drag.
        assert!(touch.feed(Moved, 7, [106.0, 56.0], 1050, MAP).is_empty());
        assert!(touch.poll(1100).is_empty());
        assert_eq!(
            touch.feed(Ended, 7, [106.0, 56.0], 1200, MAP),
            [press(Left), release(Left)]
        );
        assert!(!touch.active());
        // Moves and ends of unknown fingers are nothing.
        assert!(touch.feed(Moved, 9, [1.0, 1.0], 1300, MAP).is_empty());
        assert!(touch.feed(Ended, 9, [1.0, 1.0], 1300, MAP).is_empty());
    }

    #[test]
    fn a_drag_past_the_slop_pans_with_the_middle_button_in_the_camera_view() {
        let mut touch = TouchGestures::default();
        touch.feed(Started, 1, [100.0, 100.0], 0, CAMERA);
        assert!(touch.feed(Moved, 1, [108.0, 100.0], 16, CAMERA).is_empty());
        // Past the slop: the middle button goes down where the finger
        // landed (the pointer is still there), then the pointer follows.
        assert_eq!(
            touch.feed(Moved, 1, [112.0, 100.0], 32, CAMERA),
            [press(Middle), TouchAction::Pointer([112.0, 100.0])]
        );
        assert_eq!(
            touch.feed(Moved, 1, [160.0, 130.0], 48, CAMERA),
            [TouchAction::Pointer([160.0, 130.0])]
        );
        // A drag never long-presses, and its lift only ends the pan.
        assert!(touch.poll(2000).is_empty());
        assert_eq!(
            touch.feed(Ended, 1, [160.0, 130.0], 2000, CAMERA),
            [release(Middle)]
        );
        // Without the camera (Vanilla, the fixed view) it moves the pointer
        // and clicks nothing.
        let mut touch = TouchGestures::default();
        touch.feed(Started, 1, [100.0, 100.0], 0, MAP);
        assert_eq!(
            touch.feed(Moved, 1, [100.0, 140.0], 16, MAP),
            [TouchAction::Pointer([100.0, 140.0])]
        );
        assert!(touch.feed(Ended, 1, [100.0, 140.0], 32, MAP).is_empty());
    }

    #[test]
    fn the_slop_scales_with_the_window() {
        let mut touch = TouchGestures::new(SLOP * 2.0);
        touch.feed(Started, 1, [0.0, 0.0], 0, CAMERA);
        assert!(touch.feed(Moved, 1, [15.0, 0.0], 16, CAMERA).is_empty());
        assert_eq!(
            touch.feed(Ended, 1, [15.0, 0.0], 32, CAMERA),
            [press(Left), release(Left)]
        );
    }

    #[test]
    fn a_hold_presses_the_left_button_for_the_action_menu_and_the_lift_picks() {
        assert_eq!(HOLD_PRESS_MS + MENU_HOLD_MS, LONG_PRESS_MS);
        let mut touch = TouchGestures::default();
        touch.feed(Started, 3, [200.0, 200.0], 1000, CAMERA);
        assert!(touch.poll(1000 + HOLD_PRESS_MS - 1).is_empty());
        // Still for HOLD_PRESS_MS: the left button goes down and stays
        // down, so the game's held-button pulse opens the menu by
        // LONG_PRESS_MS.
        assert_eq!(touch.poll(1000 + HOLD_PRESS_MS), [press(Left)]);
        assert!(touch.poll(1600).is_empty());
        // With the menu open the finger slides through its entries, past
        // the slop too, and the lift releases there (the pick).
        for y in [230.0, 290.0] {
            assert_eq!(
                touch.feed(Moved, 3, [201.0, y], 1700, MENU),
                [TouchAction::Pointer([201.0, y])]
            );
        }
        // A second finger does not interrupt the open menu.
        assert!(
            touch
                .feed(Started, 4, [400.0, 200.0], 1750, MENU)
                .is_empty()
        );
        assert!(touch.feed(Ended, 4, [400.0, 200.0], 1760, MENU).is_empty());
        assert_eq!(
            touch.feed(Ended, 3, [201.0, 290.0], 1800, MENU),
            [TouchAction::Pointer([201.0, 290.0]), release(Left)]
        );
        assert!(!touch.active());
        // Lifted before any menu (nothing under it): the hold is a click.
        touch.feed(Started, 5, [50.0, 50.0], 2000, CAMERA);
        assert_eq!(touch.poll(2300), [press(Left)]);
        assert_eq!(
            touch.feed(Ended, 5, [52.0, 50.0], 2400, CAMERA),
            [TouchAction::Pointer([52.0, 50.0]), release(Left)]
        );
        // A lift the frames did not poll in time is a click.
        touch.feed(Started, 6, [200.0, 200.0], 3000, CAMERA);
        assert_eq!(
            touch.feed(Ended, 6, [200.0, 200.0], 3600, CAMERA),
            [press(Left), release(Left)]
        );
        // A long press is never a right click any more.
        touch.feed(Started, 7, [200.0, 200.0], 4000, MAP);
        for now in (4000..5000).step_by(16) {
            assert!(
                touch
                    .poll(now)
                    .iter()
                    .all(|action| !matches!(action, TouchAction::Button { button: Right, .. }))
            );
        }
        touch.feed(Ended, 7, [200.0, 200.0], 5000, MAP);
    }

    #[test]
    fn a_hold_without_a_menu_gives_way_to_a_drag_or_two_fingers() {
        // Held, then slid past the slop with no menu open (empty floor):
        // the press is withdrawn, released outside so it does not act, and
        // the camera pans from where the finger landed.
        let mut touch = TouchGestures::default();
        touch.feed(Started, 1, [100.0, 100.0], 0, CAMERA);
        assert_eq!(touch.poll(HOLD_PRESS_MS), [press(Left)]);
        assert_eq!(
            touch.feed(Moved, 1, [104.0, 100.0], 260, CAMERA),
            [TouchAction::Pointer([104.0, 100.0])]
        );
        assert_eq!(
            touch.feed(Moved, 1, [140.0, 100.0], 270, CAMERA),
            [
                TouchAction::Leave,
                release(Left),
                TouchAction::Pointer([100.0, 100.0]),
                press(Middle),
                TouchAction::Pointer([140.0, 100.0]),
            ]
        );
        assert_eq!(
            touch.feed(Ended, 1, [140.0, 100.0], 400, CAMERA),
            [release(Middle)]
        );
        // Without the camera the slide only moves the pointer.
        touch.feed(Started, 1, [100.0, 100.0], 1000, MAP);
        touch.poll(1000 + HOLD_PRESS_MS);
        assert_eq!(
            touch.feed(Moved, 1, [100.0, 140.0], 1300, MAP),
            [
                TouchAction::Leave,
                release(Left),
                TouchAction::Pointer([100.0, 140.0])
            ]
        );
        assert!(touch.feed(Ended, 1, [100.0, 140.0], 1400, MAP).is_empty());
        // A second finger withdraws the press too, and can still tap.
        touch.feed(Started, 1, [100.0, 100.0], 2000, CAMERA);
        touch.poll(2000 + HOLD_PRESS_MS);
        assert_eq!(
            touch.feed(Started, 2, [200.0, 100.0], 2300, CAMERA),
            [
                TouchAction::Leave,
                release(Left),
                TouchAction::Pointer([100.0, 100.0])
            ]
        );
        assert_eq!(
            touch.feed(Ended, 2, [200.0, 100.0], 2350, CAMERA),
            [press(Right), release(Right)]
        );
        touch.feed(Ended, 1, [100.0, 100.0], 2360, CAMERA);
        // Cancelled: released outside the window.
        touch.feed(Started, 1, [100.0, 100.0], 3000, CAMERA);
        touch.poll(3000 + HOLD_PRESS_MS);
        assert_eq!(
            touch.feed(Cancelled, 1, [100.0, 100.0], 3300, CAMERA),
            [TouchAction::Leave, release(Left)]
        );
        assert!(!touch.active());
    }

    #[test]
    fn a_two_finger_tap_right_clicks_and_a_slow_or_moved_one_does_not() {
        let mut touch = TouchGestures::default();
        touch.feed(Started, 1, [100.0, 100.0], 0, CAMERA);
        assert!(
            touch
                .feed(Started, 2, [200.0, 100.0], 40, CAMERA)
                .is_empty()
        );
        // The second finger stops the long press.
        assert!(touch.poll(600).is_empty());
        assert_eq!(
            touch.feed(Ended, 2, [200.0, 100.0], 120, CAMERA),
            [press(Right), release(Right)]
        );
        // The other finger's lift is nothing.
        assert!(touch.feed(Ended, 1, [100.0, 100.0], 130, CAMERA).is_empty());
        assert!(!touch.active());
        // Held past the long press: no tap.
        touch.feed(Started, 1, [100.0, 100.0], 1000, CAMERA);
        touch.feed(Started, 2, [200.0, 100.0], 1010, CAMERA);
        assert!(
            touch
                .feed(Ended, 1, [100.0, 100.0], 1600, CAMERA)
                .is_empty()
        );
        touch.feed(Ended, 2, [200.0, 100.0], 1610, CAMERA);
        // Two fingers sliding together (no spread change): no tap, no zoom.
        touch.feed(Started, 1, [100.0, 100.0], 2000, CAMERA);
        touch.feed(Started, 2, [200.0, 100.0], 2010, CAMERA);
        assert!(
            touch
                .feed(Moved, 1, [100.0, 140.0], 2020, CAMERA)
                .is_empty()
        );
        assert!(
            touch
                .feed(Moved, 2, [200.0, 140.0], 2030, CAMERA)
                .is_empty()
        );
        assert!(
            touch
                .feed(Ended, 1, [100.0, 140.0], 2040, CAMERA)
                .is_empty()
        );
        touch.feed(Ended, 2, [200.0, 140.0], 2050, CAMERA);
        assert!(!touch.active());
    }

    #[test]
    fn a_pinch_zooms_by_the_spread_about_the_midpoint() {
        let mut touch = TouchGestures::default();
        touch.feed(Started, 1, [400.0, 300.0], 0, CAMERA);
        touch.feed(Started, 2, [500.0, 300.0], 10, CAMERA);
        // Within the slop: nothing yet.
        assert!(touch.feed(Moved, 2, [508.0, 300.0], 20, CAMERA).is_empty());
        // Spread to 115 pixels: one zoom step, measured from the spread
        // when the second finger landed, at the midpoint.
        let out = touch.feed(Moved, 2, [515.0, 300.0], 30, CAMERA);
        let [TouchAction::Pointer(mid), TouchAction::Zoom { lines }] = out[..] else {
            panic!("{out:?}");
        };
        assert_eq!(mid, [457.5, 300.0]);
        assert!((lines - 1.0).abs() < 1e-9, "{lines}");
        // Each further change is relative to the last zoom sent: the lines
        // add up to the whole spread ratio, both ways.
        let mut total = lines;
        for (x, now) in [(600.0, 40), (700.0, 50), (430.0, 60)] {
            for action in touch.feed(Moved, 2, [x, 300.0], now, CAMERA) {
                if let TouchAction::Zoom { lines } = action {
                    total += lines;
                }
            }
        }
        let expected = (30.0f64 / 100.0).ln() / ZOOM_STEP.ln();
        assert!((total - expected).abs() < 1e-9, "{total} vs {expected}");
        // A pinch never taps on its lift.
        assert!(touch.feed(Ended, 1, [400.0, 300.0], 70, CAMERA).is_empty());
        assert!(touch.feed(Ended, 2, [430.0, 300.0], 80, CAMERA).is_empty());
        // Without the camera a pinch sends nothing at all.
        touch.feed(Started, 1, [400.0, 300.0], 100, MAP);
        touch.feed(Started, 2, [500.0, 300.0], 110, MAP);
        assert!(touch.feed(Moved, 2, [700.0, 300.0], 120, MAP).is_empty());
        assert!(touch.feed(Ended, 2, [700.0, 300.0], 130, MAP).is_empty());
        assert!(touch.feed(Ended, 1, [400.0, 300.0], 140, MAP).is_empty());
    }

    #[test]
    fn a_second_finger_ends_a_drag_and_a_third_is_ignored() {
        let mut touch = TouchGestures::default();
        touch.feed(Started, 1, [100.0, 100.0], 0, CAMERA);
        touch.feed(Moved, 1, [150.0, 100.0], 10, CAMERA);
        assert_eq!(
            touch.feed(Started, 2, [250.0, 100.0], 20, CAMERA),
            [release(Middle)]
        );
        assert!(touch.feed(Started, 3, [0.0, 0.0], 30, CAMERA).is_empty());
        // The pinch goes on from the drag's place; the third finger's
        // moves count for nothing.
        assert!(touch.feed(Moved, 3, [50.0, 50.0], 40, CAMERA).is_empty());
        let out = touch.feed(Moved, 2, [350.0, 100.0], 50, CAMERA);
        assert!(matches!(
            out[..],
            [TouchAction::Pointer(_), TouchAction::Zoom { .. }]
        ));
        // Lifting a pinching finger ends the gesture even with two down.
        assert!(touch.feed(Ended, 2, [350.0, 100.0], 60, CAMERA).is_empty());
        assert!(touch.feed(Moved, 1, [0.0, 100.0], 70, CAMERA).is_empty());
        assert!(touch.feed(Ended, 1, [0.0, 100.0], 80, CAMERA).is_empty());
        assert!(touch.active(), "the third finger is still down");
        assert!(touch.feed(Ended, 3, [50.0, 50.0], 90, CAMERA).is_empty());
        assert!(!touch.active());
        // A fresh tap works again.
        touch.feed(Started, 4, [10.0, 10.0], 100, CAMERA);
        assert_eq!(
            touch.feed(Ended, 4, [10.0, 10.0], 150, CAMERA),
            [press(Left), release(Left)]
        );
    }

    #[test]
    fn an_on_screen_button_presses_at_once_and_releases_on_the_lift() {
        let mut touch = TouchGestures::default();
        assert_eq!(
            touch.feed(Started, 1, [600.0, 20.0], 0, BUTTON),
            [TouchAction::Pointer([600.0, 20.0]), press(Left)]
        );
        // Held: no long press, and a slide only moves the pointer.
        assert!(touch.poll(900).is_empty());
        assert_eq!(
            touch.feed(Moved, 1, [604.0, 22.0], 950, BUTTON),
            [TouchAction::Pointer([604.0, 22.0])]
        );
        assert_eq!(
            touch.feed(Ended, 1, [604.0, 22.0], 1000, BUTTON),
            [TouchAction::Pointer([604.0, 22.0]), release(Left)]
        );
        // Only the first finger's landing counts as a button.
        touch.feed(Started, 1, [100.0, 100.0], 2000, CAMERA);
        assert!(
            touch
                .feed(Started, 2, [600.0, 20.0], 2010, BUTTON)
                .is_empty()
        );
    }

    #[test]
    fn a_cancelled_touch_ends_its_gesture_without_acting() {
        let mut touch = TouchGestures::default();
        // A pending tap: nothing.
        touch.feed(Started, 1, [100.0, 100.0], 0, CAMERA);
        assert!(
            touch
                .feed(Cancelled, 1, [100.0, 100.0], 50, CAMERA)
                .is_empty()
        );
        assert!(!touch.active());
        // A drag: its middle button comes up.
        touch.feed(Started, 1, [100.0, 100.0], 100, CAMERA);
        touch.feed(Moved, 1, [200.0, 100.0], 110, CAMERA);
        assert_eq!(
            touch.feed(Cancelled, 1, [200.0, 100.0], 120, CAMERA),
            [release(Middle)]
        );
        // A held button: released outside the window, so it does not act.
        touch.feed(Started, 1, [600.0, 20.0], 200, BUTTON);
        assert_eq!(
            touch.feed(Cancelled, 1, [600.0, 20.0], 210, BUTTON),
            [TouchAction::Leave, release(Left)]
        );
        // A two-finger tap: nothing.
        touch.feed(Started, 1, [100.0, 100.0], 300, CAMERA);
        touch.feed(Started, 2, [200.0, 100.0], 310, CAMERA);
        assert!(
            touch
                .feed(Cancelled, 2, [200.0, 100.0], 320, CAMERA)
                .is_empty()
        );
        assert!(touch.feed(Ended, 1, [100.0, 100.0], 330, CAMERA).is_empty());
        assert!(!touch.active());
    }

    #[test]
    fn actions_are_the_mouse_records() {
        assert_eq!(
            TouchAction::Pointer([1.5, 2.0]).command(),
            InputCommand::PointerMoved { x: 1.5, y: 2.0 }
        );
        assert_eq!(TouchAction::Leave.command(), InputCommand::PointerLeft);
        assert_eq!(
            press(Middle).command(),
            InputCommand::PointerButton {
                button: "Middle".into(),
                pressed: true
            }
        );
        assert_eq!(
            TouchAction::Zoom { lines: -0.5 }.command(),
            InputCommand::Wheel {
                x: 0.0,
                y: -0.5,
                unit: WheelUnit::Lines
            }
        );
    }
}
