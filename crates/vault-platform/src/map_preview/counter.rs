//! SPEC-013 §3.17 the move-items counter (`movemult.frm` at screen (140,
//! 80)), shared by the loot, barter and inventory windows: the title, the
//! item picture, three digits, plus, minus, ALL, Done and Cancel.
use crate::{input::key_digit, interface::Painter};
use vault_core::{
    skills::{self, Counter},
    ui::{ColorKey, Rect},
};
use vault_formats::frm::FrmFrame;
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

const PLUS: Rect = Rect::new(200, 46, 16, 12);
const MINUS: Rect = Rect::new(200, 58, 17, 12);
const ALL: Rect = Rect::new(120, 80, 94, 33);
const DONE: Rect = Rect::new(98, 128, 15, 16);
const CANCEL: Rect = Rect::new(148, 128, 15, 16);

/// What an input did to the open counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CounterInput {
    /// Enter or Done with a value in range: move this many.
    Accept(i32),
    /// Enter or Done with a value out of range (`iisxxxx1`); it stays open.
    Refused,
    /// Esc or Cancel: nothing moves.
    Cancel,
    /// A button or digit changed the value.
    Changed,
    /// Not for the counter.
    Ignored,
}

/// The counter button under a screen point, as a screen rectangle (for its
/// down art while held).
pub(crate) fn counter_button(point: [i32; 2]) -> Option<Rect> {
    let [cx, cy] = skills::COUNTER_ORIGIN;
    [PLUS, MINUS, ALL, DONE, CANCEL]
        .into_iter()
        .map(|rect| Rect::new(cx + rect.x, cy + rect.y, rect.w, rect.h))
        .find(|rect| rect.contains(point))
}

/// The counter, its buttons in their down art while `held` (the held
/// button's screen rectangle) is over them. ALL's up image is
/// `allbon.frm` with its caption in key 21091, its down image `allboff.frm`
/// with key 18977 (SPEC-013 §3.17).
pub(crate) fn draw_counter(
    painter: &Painter,
    image: &mut IndexedImage,
    counter: &Counter,
    picture: Option<&FrmFrame>,
    held: Option<Rect>,
) {
    let [cx, cy] = skills::COUNTER_ORIGIN;
    painter.blit_opaque(image, "movemult.frm", 0, [cx, cy]);
    let title = painter.message("inventry.msg", 21);
    painter.centered(image, 103, ColorKey::AMBER, &title, [cx, cy + 9], 259);
    if let Some(frame) = picture {
        image.draw_frame(frame, [(cx + 16) as u32, (cy + 46) as u32], [90, 61]);
    }
    for (digit, [x, y]) in counter.digits().iter().zip(skills::COUNTER_DIGITS) {
        painter.big_digit(image, *digit, [cx + x, cy + y], false);
    }
    let down = |rect: Rect| held == Some(Rect::new(cx + rect.x, cy + rect.y, rect.w, rect.h));
    let art = |rect: Rect, up: &'static str, pressed: &'static str| {
        if down(rect) { pressed } else { up }
    };
    painter.blit(
        image,
        art(PLUS, "splsoff.frm", "splson.frm"),
        0,
        [cx + PLUS.x, cy + PLUS.y],
    );
    painter.blit(
        image,
        art(MINUS, "snegoff.frm", "snegon.frm"),
        0,
        [cx + MINUS.x, cy + MINUS.y],
    );
    painter.blit(
        image,
        art(ALL, "allbon.frm", "allboff.frm"),
        0,
        [cx + ALL.x, cy + ALL.y],
    );
    let all = painter.message("inventry.msg", 22);
    let key = if down(ALL) {
        ColorKey(18977)
    } else {
        ColorKey::AMBER
    };
    painter.centered(image, 103, key, &all, [cx + ALL.x, cy + ALL.y + 4], 94);
    for rect in [DONE, CANCEL] {
        let red = art(rect, "lilredup.frm", "lilreddn.frm");
        painter.blit(image, red, 0, [cx + rect.x, cy + rect.y]);
    }
}

fn accept(counter: &Counter) -> CounterInput {
    counter
        .enter()
        .map_or(CounterInput::Refused, CounterInput::Accept)
}

/// A press at a screen point.
pub(crate) fn counter_click(counter: &mut Counter, point: [i32; 2]) -> CounterInput {
    let [cx, cy] = skills::COUNTER_ORIGIN;
    let local = [point[0] - cx, point[1] - cy];
    if PLUS.contains(local) {
        counter.plus();
    } else if MINUS.contains(local) {
        counter.minus();
    } else if ALL.contains(local) {
        counter.all();
    } else if DONE.contains(local) {
        return accept(counter);
    } else if CANCEL.contains(local) {
        return CounterInput::Cancel;
    } else {
        return CounterInput::Ignored;
    }
    CounterInput::Changed
}

pub(crate) fn counter_key(counter: &mut Counter, key: KeyCode) -> CounterInput {
    match key {
        KeyCode::Escape => CounterInput::Cancel,
        KeyCode::Enter | KeyCode::NumpadEnter => accept(counter),
        KeyCode::Backspace => {
            counter.backspace();
            CounterInput::Changed
        }
        _ => {
            let Some(digit) = key_digit(key) else {
                return CounterInput::Ignored;
            };
            counter.digit(digit - b'0');
            CounterInput::Changed
        }
    }
}
