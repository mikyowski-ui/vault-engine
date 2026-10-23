//! SPEC-014 §4.18 Fixture R: the dialogue panel, Review layout, wrapping
//! and scrolling, and the Ask About window and input rules.
use super::*;

/// A synthetic font: every glyph 6 wide plus 1 spacing.
fn measure(text: &[u8]) -> i32 {
    7 * text.len() as i32
}

fn entry(reply: &[u8], choice: Option<&[u8]>) -> ReviewText {
    ReviewText {
        reply: reply.to_vec(),
        choice: choice.map(<[u8]>::to_vec),
    }
}

#[test]
fn spec014_fixture_r_panel_and_windows() {
    assert_eq!(panel_button(REVIEW), Rect::new(13, 444, 51, 29));
    assert_eq!(panel_button(BARTER), Rect::new(593, 331, 14, 14));
    assert_eq!(panel_button(TELL_ME_ABOUT), Rect::new(593, 406, 14, 14));
    assert_eq!(ASK_ORIGIN, [175, 356]);
    assert_eq!(FIELD, Rect::new(22, 32, 244, 14));
    // The corpus font3 title is 107 wide: centered at x = 91.
    assert_eq!(super::super::centered(ASK_SIZE[0], 107), 91);
}

#[test]
fn spec014_fixture_r_review_layout() {
    let lines = review_lines(
        &[
            entry(b"Hng.", Some(b"\x95 Who are you?")),
            entry(b"Bye.", None),
        ],
        0,
        b"Abel",
        b"Max",
        10,
        measure,
    );
    let at = |line: &ReviewLine| (line.text.clone(), line.origin, line.key);
    assert_eq!(
        lines.iter().map(at).collect::<Vec<_>>(),
        [
            (b"Abel:".to_vec(), [88, 76], TARGET_NAME_KEY),
            (b"Hng.".to_vec(), [123, 88], REPLY_KEY),
            (b"Max:".to_vec(), [88, 98], PLAYER_NAME_KEY),
            (b"\x95 Who are you?".to_vec(), [123, 110], OPTION_KEY),
            (b"Abel:".to_vec(), [88, 120], TARGET_NAME_KEY),
            (b"Bye.".to_vec(), [123, 132], REPLY_KEY),
        ]
    );
    assert_eq!((lines[0].limit, lines[1].limit), (NAME_LIMIT, TEXT_WIDTH));
    // A reply needing two lines: the second at (113, top + 10).
    let long = [b"word ".repeat(12).as_slice(), b"end"].concat();
    let lines = review_lines(&[entry(&long, None)], 0, b"Abel", b"Max", 10, measure);
    assert_eq!(lines[1].origin, [123, 88]);
    assert_eq!(lines[2].origin, [113, 98]);
    // Drawing stops after the first entry that leaves y at 407 or more,
    // and no line starts below 408.
    let many = vec![entry(b"Hng.", Some(b"Ok")); 20];
    let lines = review_lines(&many, 0, b"Abel", b"Max", 10, measure);
    // Entries take 44 pixels: the eighth starts at 384, and its option
    // (top 418) is cut, leaving the player's name at the bottom.
    assert!(lines.iter().all(|line| line.origin[1] <= LAST_LINE_TOP));
    assert_eq!(lines.len(), 4 * 7 + 3);
    let last = lines.last().unwrap();
    assert_eq!(
        (last.text.as_slice(), last.origin),
        (&b"Max:"[..], [88, 406])
    );
}

#[test]
fn spec014_review_wrapping() {
    // At most the width: one line.
    assert_eq!(wrap(b"one two", 49, measure), [&b"one two"[..]]);
    // Breaks extend while the text up to the next space is narrower.
    assert_eq!(wrap(b"aa bb cc dd", 42, measure), [&b"aa bb"[..], b"cc dd"]);
    // Exactly the width at a later space does not extend.
    assert_eq!(wrap(b"aa bbb cc", 42, measure), [&b"aa"[..], b"bbb cc"]);
    // No space after the first character: the rest is one line, cut when
    // drawn, and the text ends.
    assert_eq!(wrap(b"abcdefghij", 42, measure), [&b"abcdefghij"[..]]);
    assert_eq!(wrap(b" abcdefghij", 42, measure), [&b" abcdefghij"[..]]);
    // A first word wider than the width stops the text (and the stored
    // text is untouched: these are borrowed slices).
    assert_eq!(wrap(b"ok abcdefghij more", 42, measure), [&b"ok"[..]]);
    assert!(wrap(b"", 42, measure).is_empty());
}

#[test]
fn spec014_fixture_r_review_scrolling() {
    // 5 entries, Down 6 times: 1, 2, 3, 4, then 4 twice with no redraw.
    let mut top = 0;
    let mut seen = Vec::new();
    for _ in 0..6 {
        match scroll(top, 5, true) {
            Some(next) => {
                top = next;
                seen.push(Some(top));
            }
            None => seen.push(None),
        }
    }
    assert_eq!(seen, [Some(1), Some(2), Some(3), Some(4), None, None]);
    assert_eq!(scroll(0, 5, false), None, "Up at entry 0: no redraw");
    assert_eq!(scroll(3, 5, false), Some(2));
    assert_eq!(scroll(0, 0, true), None);
}

#[test]
fn spec014_fixture_r_ask_about_input() {
    let mut field = AskField::new(1000);
    assert_eq!(field.shown(), b"_");
    // The cursor blinks only past 333 ms.
    assert!(!field.blink(1333));
    assert!(field.blink(1334));
    assert_eq!(field.shown(), b" ");
    // Typed text 230 wide with `_`, next glyph 9: 239 < 244, accepted;
    // 236 wide and glyph 8: 244 is not below 244, refused.
    let fixed = |width: i32| move |_: &[u8]| width;
    assert!(field.type_code(i32::from(b'a'), fixed(230), |_| 9));
    assert!(!field.type_code(i32::from(b'b'), fixed(236), |_| 8));
    assert_eq!(field.text, b"a");
    // Safe rule: Up arrow (328) and control codes are ignored.
    assert!(!field.type_code(328, measure, |_| 6));
    assert!(!field.type_code(8, measure, |_| 6));
    assert!(field.type_code(255, measure, |_| 6));
    // At most 125 characters.
    let mut full = AskField::new(0);
    for _ in 0..200 {
        full.type_code(i32::from(b'i'), |_| 0, |_| 0);
    }
    assert_eq!(full.text.len(), TYPED_LIMIT);
    full.backspace();
    assert_eq!(full.text.len(), TYPED_LIMIT - 1);
    // Enter submits and clears; an empty field submits nothing.
    assert_eq!(full.submit().map(|text| text.len()), Some(TYPED_LIMIT - 1));
    assert!(full.text.is_empty());
    assert_eq!(full.submit(), None);
}
