//! The world-map view state both editions share (SPEC-014 §3.39, kept by
//! SPEC-019 §3.12): the view origin, follow mode, the pause window and its
//! grey, the party hover, the travel dots, the held side-panel button and
//! the pending timed step.
use vault_core::{
    ui::{cursor, fallout2::view as view2},
    worldmap::screen,
};
use winit::keyboard::KeyCode;

/// The world a view scrolls over: the one place the view geometry
/// branches on the edition (SPEC-020 §2.2 rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bounds {
    /// SPEC-014 §3.39 the fixed 1400-by-1500 `worldmap.frm`.
    Fallout1,
    /// SPEC-019 §3.12.1 a world of `world` pixels (SPEC-018 §5.3).
    Fallout2 { world: [i32; 2] },
}
impl Bounds {
    /// The view centered on world position `position`, clamped.
    pub(crate) fn centered(self, position: [i32; 2]) -> [i32; 2] {
        match self {
            Self::Fallout1 => screen::centered(position),
            Self::Fallout2 { world } => view2::centered(position, world),
        }
    }
    /// One scroll step, and whether every requested axis was at its limit.
    pub(crate) fn scroll(self, view: [i32; 2], direction: [i32; 2]) -> ([i32; 2], bool) {
        match self {
            Self::Fallout1 => screen::scroll(view, direction),
            Self::Fallout2 { world } => view2::scroll(view, direction, world),
        }
    }
    /// The Home key's view.
    pub(crate) fn home(self, position: [i32; 2]) -> [i32; 2] {
        match self {
            Self::Fallout1 => screen::home(position),
            Self::Fallout2 { world } => view2::home(position, world),
        }
    }
    /// The End key's view.
    pub(crate) fn end(self, view: [i32; 2]) -> [i32; 2] {
        match self {
            Self::Fallout1 => screen::end(view),
            Self::Fallout2 { world } => view2::end(view, world),
        }
    }
}

/// Travel dots: one bit per world pixel. A pixel outside the world (or
/// past the last word) is never marked.
#[derive(Debug, Clone)]
struct TravelDots {
    words: Vec<u64>,
    width: i32,
}
impl TravelDots {
    fn new(size: [i32; 2]) -> Self {
        let pixels = (size[0].max(1) as usize) * (size[1].max(1) as usize);
        Self {
            words: vec![0; pixels / 64 + 1],
            width: size[0].max(1),
        }
    }
    fn index(&self, pixel: [i32; 2]) -> Option<usize> {
        let height = (self.words.len() * 64) as i32 / self.width;
        ((0..self.width).contains(&pixel[0]) && (0..height).contains(&pixel[1]))
            .then(|| (pixel[1] * self.width + pixel[0]) as usize)
            .filter(|index| *index / 64 < self.words.len())
    }
    fn mark(&mut self, pixel: [i32; 2]) {
        if let Some(index) = self.index(pixel) {
            self.words[index / 64] |= 1 << (index % 64);
        }
    }
    fn marked(&self, pixel: [i32; 2]) -> bool {
        self.index(pixel)
            .is_some_and(|index| self.words[index / 64] & (1 << (index % 64)) != 0)
    }
    fn clear(&mut self) {
        self.words.fill(0);
    }
}

/// What a world-map key asks of the shell, after the view took its part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyOutcome {
    Done,
    Brightness(bool),
    Quit,
    Screenshot,
}

/// The world-map view state, with the side-panel button type `B` and the
/// pending step type `P` of its edition.
pub(crate) struct WorldView<B, P> {
    /// The world pixel shown at (22, 21).
    pub view: [i32; 2],
    pub follow: bool,
    /// G-24 / SPEC-014 §3.17 the world-map pause window is open.
    pub paused: bool,
    /// §3.17 the pause greyed the view: the view origin, follow mode and
    /// hover state when it did. The region stays grey until a pass
    /// redraws the view (§3.39 "Drawing a pass": follow mode on, a scroll,
    /// a hover change or a travel iteration).
    pub grey: Option<([i32; 2], bool, bool)>,
    /// The pointer hovers on the party (at rest).
    pub hover: bool,
    dots: TravelDots,
    /// The town or city a journey chosen on a town map heads for.
    pub journey: Option<usize>,
    /// The start of the last pass.
    last_pass: Option<u64>,
    /// The side-panel button held down.
    held: Option<B>,
    /// The press closed the pause window: its release does nothing.
    swallow: bool,
    pub pending: Option<P>,
}

impl<B: Copy, P> WorldView<B, P> {
    /// A view at `view` over a world of `size` pixels, follow mode on.
    pub(crate) fn new(view: [i32; 2], size: [i32; 2]) -> Self {
        Self {
            view,
            follow: true,
            paused: false,
            grey: None,
            hover: false,
            dots: TravelDots::new(size),
            journey: None,
            last_pass: None,
            held: None,
            swallow: false,
            pending: None,
        }
    }
    pub(crate) fn mark(&mut self, pixel: [i32; 2]) {
        self.dots.mark(pixel);
    }
    pub(crate) fn marked(&self, pixel: [i32; 2]) -> bool {
        self.dots.marked(pixel)
    }
    pub(crate) fn clear_dots(&mut self) {
        self.dots.clear();
    }
    /// The next left release does nothing: its press went to a window in
    /// front (a dialog box) or closed the pause window.
    pub(crate) fn swallow_release(&mut self) {
        self.swallow = true;
    }
    /// The side-panel button held down.
    pub(crate) fn held(&self) -> Option<B> {
        self.held
    }
    /// Building the screen (SPEC-014 §3.39): no travel dots, no pass yet,
    /// nothing held or pending, no hover.
    pub(crate) fn rebuild(&mut self) {
        self.clear_dots();
        self.last_pass = None;
        self.held = None;
        self.pending = None;
        self.hover = false;
    }
    /// A key on the world map. G-24 / SPEC-014 §3.17: `p` opens the pause
    /// window; it closes on any key except the brightness keys. A pending
    /// step takes every key. No key closes the world map; Esc does nothing.
    pub(crate) fn key(
        &mut self,
        key: KeyCode,
        control: bool,
        bounds: Bounds,
        position: [i32; 2],
    ) -> KeyOutcome {
        if self.paused {
            return match key {
                KeyCode::Equal | KeyCode::NumpadAdd => KeyOutcome::Brightness(true),
                KeyCode::Minus | KeyCode::NumpadSubtract => KeyOutcome::Brightness(false),
                _ => {
                    self.paused = false;
                    KeyOutcome::Done
                }
            };
        }
        if self.pending.is_some() {
            return KeyOutcome::Done;
        }
        match key {
            KeyCode::KeyP => {
                self.paused = true;
                self.grey = Some((self.view, self.follow, self.hover));
            }
            KeyCode::KeyA => self.follow = !self.follow,
            KeyCode::KeyC => self.view = bounds.centered(position),
            KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::ArrowUp | KeyCode::ArrowDown => {
                let direction = match key {
                    KeyCode::ArrowLeft => [-1, 0],
                    KeyCode::ArrowRight => [1, 0],
                    KeyCode::ArrowUp => [0, -1],
                    _ => [0, 1],
                };
                // §3.39: a key is ignored when the view is already at that
                // limit, so it leaves follow mode as it is.
                let (view, blocked) = bounds.scroll(self.view, direction);
                if !blocked {
                    self.follow = false;
                    self.view = view;
                }
            }
            KeyCode::Home => {
                self.follow = false;
                self.view = bounds.home(position);
            }
            KeyCode::End => {
                self.follow = false;
                self.view = bounds.end(self.view);
            }
            KeyCode::KeyQ | KeyCode::KeyX if control => return KeyOutcome::Quit,
            KeyCode::F10 => return KeyOutcome::Quit,
            KeyCode::F12 => return KeyOutcome::Screenshot,
            KeyCode::Equal | KeyCode::NumpadAdd => return KeyOutcome::Brightness(true),
            KeyCode::Minus | KeyCode::NumpadSubtract => return KeyOutcome::Brightness(false),
            _ => {}
        }
        KeyOutcome::Done
    }
    /// A press on the world map: a press closes the pause window (its left
    /// release is then swallowed); a new left press owns the next release;
    /// a side-panel button (`button`, asked only when it can take the
    /// press) is held until its release. Returns whether a button went
    /// down, so the shell plays its press cue.
    pub(crate) fn press(&mut self, right: bool, button: impl FnOnce() -> Option<B>) -> bool {
        if self.paused {
            self.paused = false;
            self.swallow = !right;
            return false;
        }
        if !right {
            self.swallow = false;
        }
        if right || self.pending.is_some() {
            return false;
        }
        match button() {
            Some(button) => {
                self.held = Some(button);
                true
            }
            None => false,
        }
    }
    /// A left release the view takes whole: a swallowed one, or one while
    /// paused or pending.
    pub(crate) fn release_taken(&mut self) -> bool {
        std::mem::take(&mut self.swallow) || self.paused || self.pending.is_some()
    }
    pub(crate) fn take_held(&mut self) -> Option<B> {
        self.held.take()
    }
    /// The start of a pass at `now`, at most one per `PASS_MS` and none
    /// while paused; then edge scrolling for a pointer at `edge_pointer`
    /// (H3: none for a pointer in the letterbox). Returns whether the pass
    /// runs.
    pub(crate) fn begin_pass(
        &mut self,
        now: u64,
        edge_pointer: Option<[i32; 2]>,
        bounds: Bounds,
    ) -> bool {
        if self.paused
            || self
                .last_pass
                .is_some_and(|last| now < last + screen::PASS_MS)
        {
            return false;
        }
        self.last_pass = Some(now);
        if let Some(at) = edge_pointer {
            let direction = screen::edge(at);
            if direction != [0, 0] {
                self.follow = false;
                self.view = bounds.scroll(self.view, direction).0;
            }
        }
        true
    }
    /// The end of a pass: follow mode, the hover state, and §3.17 a pass
    /// that redraws the view ends the pause's grey.
    pub(crate) fn settle_pass(
        &mut self,
        position: [i32; 2],
        at_rest: bool,
        pointer: Option<[i32; 2]>,
        travelled: bool,
        bounds: Bounds,
    ) {
        if self.follow {
            self.view = bounds.centered(position);
        }
        let party = screen::window(position, self.view);
        self.hover = at_rest && pointer.is_some_and(|at| screen::hovers(at, party));
        if let Some((view, follow, hover)) = self.grey
            && (self.follow
                || travelled
                || view != self.view
                || follow != self.follow
                || hover != self.hover)
        {
            self.grey = None;
        }
    }
    /// The cursor: the scroll art on an edge (the blocked art when every
    /// requested axis is at its limit), else the arrow.
    pub(crate) fn cursor(&self, at: [i32; 2], bounds: Bounds) -> &'static str {
        if self.pending.is_some() || self.paused {
            return cursor::ARROW.art;
        }
        let direction = screen::edge(at);
        let Some(index) = screen::cursor_index(direction) else {
            return cursor::ARROW.art;
        };
        if bounds.scroll(self.view, direction).1 {
            cursor::SCROLL_BLOCKED[index].art
        } else {
            cursor::SCROLL[index].art
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_core::worldmap::WORLD_SIZE;

    #[test]
    fn travel_dots_cover_the_world_and_nothing_outside() {
        let mut dots = TravelDots::new(WORLD_SIZE);
        for pixel in [[0, 0], [1399, 1499], [700, 3]] {
            dots.mark(pixel);
            assert!(dots.marked(pixel));
        }
        for pixel in [[-1, 0], [1400, 0], [0, 1500], [0, -1]] {
            dots.mark(pixel);
            assert!(!dots.marked(pixel));
        }
        dots.clear();
        assert!(!dots.marked([0, 0]));
    }

    #[test]
    fn the_fallout2_bounds_match_the_fallout1_screen_on_its_world() {
        let f2 = Bounds::Fallout2 { world: WORLD_SIZE };
        for position in [[0, 0], [700, 750], [1399, 1499]] {
            assert_eq!(Bounds::Fallout1.centered(position), f2.centered(position));
            assert_eq!(Bounds::Fallout1.home(position), f2.home(position));
        }
        for direction in [[1, 0], [-1, -1], [0, 1]] {
            let view = [500, 1058];
            assert_eq!(
                Bounds::Fallout1.scroll(view, direction),
                f2.scroll(view, direction)
            );
        }
        assert_eq!(Bounds::Fallout1.end([3, 4]), f2.end([3, 4]));
    }

    #[test]
    fn a_press_while_paused_closes_the_pause_and_swallows_its_release() {
        let mut view = WorldView::<u8, ()>::new([0, 0], WORLD_SIZE);
        view.paused = true;
        assert!(!view.press(false, || Some(1)));
        assert!(!view.paused);
        assert!(view.release_taken());
        assert!(!view.release_taken());
        assert!(view.press(false, || Some(1)));
        assert_eq!(view.take_held(), Some(1));
    }
}
