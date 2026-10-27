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

/// SPEC-020 §2.1: the panel's, Review's and Ask About's layouts and
/// dispatch.
mod modal {
    use super::*;

    #[test]
    fn panel_layout_and_keys() {
        let fallout1 = panel_layout(false, false);
        assert_eq!(
            fallout1.rect(PanelControl::Barter),
            Some(Rect::new(593, 331, 14, 14))
        );
        assert_eq!(
            fallout1.rect(PanelControl::TellMeAbout),
            Some(Rect::new(593, 406, 14, 14))
        );
        assert_eq!(
            fallout1.rect(PanelControl::Review),
            Some(Rect::new(13, 444, 51, 29))
        );
        let member = panel_layout(true, true);
        assert_eq!(
            member.rect(PanelControl::CombatControl),
            Some(Rect::new(593, 406, 14, 14))
        );
        assert!(!member.contains(PanelControl::TellMeAbout));
        assert_eq!(panel_layout(true, false).len(), 2);
        assert_eq!(
            panel_key('a', false, false),
            Some(PanelControl::TellMeAbout)
        );
        assert_eq!(panel_key('a', true, true), None);
        assert_eq!(
            panel_key('c', true, true),
            Some(PanelControl::CombatControl)
        );
        assert_eq!(panel_key('c', true, false), None);
        assert_eq!(panel_key('B', false, false), Some(PanelControl::Barter));
        assert_eq!(panel_key('r', false, false), None);
        assert_eq!(PanelControl::Barter.sounds(), (SMALL_PRESS, SMALL_RELEASE));
        assert_eq!(
            PanelControl::Review.sounds(),
            (crate::ui::BUTTON_PRESS, crate::ui::BUTTON_RELEASE)
        );
        let mut panel = Panel::new(false, false);
        assert_eq!(panel.press([14, 445]), Some(PanelControl::Review));
        assert_eq!(panel.pressed(), Some(PanelControl::Review));
        assert_eq!(panel.release(Some([594, 332])), None);
        panel.press([594, 332]);
        assert_eq!(panel.release(Some([594, 332])), Some(PanelControl::Barter));
    }

    #[test]
    fn review_window_scrolls_and_closes() {
        let layout = review_layout();
        assert_eq!(
            layout.rect(ReviewControl::ScrollUp),
            Some(Rect::new(475, 152, 35, 35))
        );
        assert_eq!(
            layout.rect(ReviewControl::Done),
            Some(Rect::new(499, 398, 82, 46))
        );
        let mut review = ReviewWindow::new();
        assert_eq!(review.top, 0);
        assert_eq!(review.key(ReviewKey::Up, 3), ReviewOutcome::None);
        assert_eq!(review.key(ReviewKey::Down, 3), ReviewOutcome::Scrolled(1));
        assert_eq!(review.press([480, 195]), Some(ReviewControl::ScrollDown));
        assert_eq!(
            review.release(Some([480, 195]), 3),
            ReviewOutcome::Scrolled(2)
        );
        assert_eq!(
            review.key(ReviewKey::Down, 3),
            ReviewOutcome::None,
            "stops at the last entry"
        );
        review.press([480, 160]);
        assert_eq!(review.release(Some([0, 0]), 3), ReviewOutcome::None);
        assert_eq!(review.top, 2);
        review.press([500, 400]);
        assert_eq!(review.release(Some([500, 400]), 3), ReviewOutcome::Close);
        assert_eq!(review.key(ReviewKey::Escape, 3), ReviewOutcome::Close);
        assert_eq!(review.key(ReviewKey::Other, 3), ReviewOutcome::None);
        assert_eq!(
            ReviewControl::ScrollUp.sounds(),
            (SMALL_PRESS, SMALL_RELEASE)
        );
    }

    #[test]
    fn ask_about_window_opens_types_and_submits() {
        let layout = ask_layout();
        assert_eq!(
            layout.rect(AskControl::Done),
            Some(Rect::new(209, 414, 15, 16))
        );
        assert_eq!(
            layout.rect(AskControl::Cancel),
            Some(Rect::new(335, 414, 15, 16))
        );
        let glyph = |_: u8| 6;
        let mut ask = AskAboutWindow::new(0);
        assert_eq!(ask.hides(), (true, true));
        // Opening: input waits for the next tick.
        assert_eq!(ask.press([210, 415]), None);
        assert_eq!(ask.key(AskKey::Code(86), measure, glyph), AskOutcome::None);
        assert!(ask.tick(0));
        assert!(!ask.opening);
        assert_eq!(
            ask.key(AskKey::Code(86), measure, glyph),
            AskOutcome::Changed
        );
        assert_eq!(ask.key(AskKey::Code(7), measure, glyph), AskOutcome::None);
        assert_eq!(
            ask.key(AskKey::Enter, measure, glyph),
            AskOutcome::Submit(b"V".to_vec())
        );
        assert_eq!(
            ask.key(AskKey::Enter, measure, glyph),
            AskOutcome::None,
            "empty field"
        );
        ask.answer_shown();
        assert_eq!(ask.hides(), (false, true));
        ask.key(AskKey::Code(87), measure, glyph);
        assert_eq!(
            ask.key(AskKey::Backspace, measure, glyph),
            AskOutcome::Changed
        );
        assert!(ask.field.text.is_empty());
        ask.key(AskKey::Code(87), measure, glyph);
        assert_eq!(ask.press([210, 415]), Some(AskControl::Done));
        assert_eq!(
            ask.release(Some([210, 415])),
            AskOutcome::Submit(b"W".to_vec())
        );
        ask.press([336, 415]);
        assert_eq!(ask.release(Some([210, 415])), AskOutcome::None);
        ask.press([336, 415]);
        assert_eq!(ask.release(Some([336, 415])), AskOutcome::Cancel);
        assert_eq!(ask.key(AskKey::Escape, measure, glyph), AskOutcome::Cancel);
        // The cursor blinks after 333 ms.
        assert!(!ask.tick(333));
        assert!(ask.tick(334));
    }
}
