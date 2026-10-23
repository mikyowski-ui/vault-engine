//! SPEC-002 §3.7 language filter: the `badwords.txt` list and the rewrite.
use crate::msg::{BadWords, FILTER_ALPHABET, MsgCatalog};

fn filtered(words: &BadWords, text: &[u8], k: usize) -> (Vec<u8>, usize) {
    let mut text = text.to_vec();
    let k = words.filter_text(&mut text, k);
    (text, k)
}

#[test]
fn spec002_language_filter_fixture() {
    let words = BadWords::parse(b"darn\n");
    assert_eq!(words.entries(), [b"DARN".to_vec()]);
    assert_eq!(filtered(&words, b"oh darn", 0), (b"oh $%#&".to_vec(), 16));
    assert_eq!(
        filtered(&words, b"Darnation, darn!", 0),
        (b"!@#$ation, &%#*!".to_vec(), 19)
    );
    assert_eq!(filtered(&words, b"undarn x", 0), (b"undarn x".to_vec(), 0));
    // The cursor wraps from 28 to 0.
    assert_eq!(filtered(&words, b"darn", 27), (b"@&!@".to_vec(), 2));
    assert_eq!(FILTER_ALPHABET.len(), 29);
}

#[test]
fn spec002_language_filter_matching() {
    // Overlapping occurrences and different entries are judged against the
    // original text; later rewrites overwrite earlier ones.
    let words = BadWords::parse(b"aa\nab\n");
    // "AA" occurs at 0 (qualifies at offset 0) and at 1 (letter before);
    // "AB" at 1 has a letter before it.
    assert_eq!(filtered(&words, b"aab", 0), (b"!@b".to_vec(), 2));
    // Digits, spaces, punctuation, apostrophes and hyphens are non-letters,
    // as are bytes 0x80-0xFF, which are not folded.
    let words = BadWords::parse(b"heck");
    assert_eq!(
        filtered(&words, b"1heck-'heck'\xE9heck\xE9", 0),
        (b"1%&*@-'&!@#'\xE9%@*$\xE9".to_vec(), 27)
    );
    let words = BadWords::parse(b"\xE9t\xE9");
    assert_eq!(filtered(&words, b"\xC9T\xC9", 0).1, 0, "0xE9 is not folded");
    // A letter after an occurrence at offset 0 is not tested.
    let words = BadWords::parse(b"darn");
    assert_eq!(filtered(&words, b"darned", 0).0, b"!@#$ed");
    // Applying the filter again to filtered text changes nothing.
    let (once, _) = filtered(&words, b"oh darn", 0);
    assert_eq!(filtered(&words, &once, 5), (once.clone(), 5));
}

#[test]
fn spec002_language_filter_word_list() {
    // CRLF becomes LF; entries are untrimmed and have no comments; a final
    // line without LF counts; blank lines are ignored (safe rule).
    let words = BadWords::parse(b"one\r\n\r\n two \n# three\nfour");
    assert_eq!(
        words.entries(),
        [
            b"ONE".to_vec(),
            b" TWO ".to_vec(),
            b"# THREE".to_vec(),
            b"FOUR".to_vec()
        ]
    );
    // A line longer than 78 bytes becomes several entries; one of exactly
    // 78 leaves an empty remainder, which is ignored.
    let mut long = vec![b'x'; 80];
    long.push(b'\n');
    long.extend(vec![b'y'; 78]);
    long.extend_from_slice(b"\nz");
    let words = BadWords::parse(&long);
    let lengths: Vec<_> = words.entries().iter().map(Vec::len).collect();
    assert_eq!(lengths, [78, 2, 78, 1]);
    assert!(BadWords::parse(b"").is_empty());
    assert!(BadWords::parse(b"\n\r\n").is_empty());
}

#[test]
fn spec002_language_filter_catalog() {
    let mut catalog =
        MsgCatalog::parse(b"{20}{snd}{darn it}\n{10}{darn}{Darn}\n{30}{}{fine}").unwrap();
    let words = BadWords::parse(b"darn");
    // Ascending ID order: 10 first, then 20; sound fields are untouched.
    let k = catalog.apply_language_filter(&words, 0);
    assert_eq!(catalog.get(10).unwrap().text, b"!@#$");
    assert_eq!(catalog.get(10).unwrap().sound, b"darn");
    assert_eq!(catalog.get(20).unwrap().text, b"%&*@ it");
    assert_eq!(catalog.get(30).unwrap().text, b"fine");
    assert_eq!(k, 8);
    assert!(!catalog.is_empty() && MsgCatalog::default().is_empty());
}

/// The corpus word list loads, and filtering every corpus dialogue catalog
/// keeps each text's length and sound field.
#[test]
#[ignore = "needs the local corpus"]
fn corpus_language_filter() {
    let Some(corpus) = crate::tests::corpus_dir() else {
        return;
    };
    let root = corpus.join("fallout1-unpacked");
    let words = BadWords::parse(&std::fs::read(root.join("data/badwords.txt")).unwrap());
    // 102 line feeds and a final line without one.
    assert_eq!(words.entries().len(), 103);
    assert!(words.entries().iter().all(|word| !word.is_empty()));
    let mut changed = 0;
    // The shipped list holds French and Spanish words.
    let dialogs = ["english", "french", "spanish"]
        .into_iter()
        .flat_map(|language| std::fs::read_dir(root.join("text").join(language).join("dialog")))
        .flat_map(|entries| entries.flatten());
    for entry in dialogs {
        let mut catalog = MsgCatalog::parse(&std::fs::read(entry.path()).unwrap()).unwrap();
        let before = catalog.clone();
        catalog.apply_language_filter(&words, 0);
        for (id, message) in catalog.entries() {
            let original = before.get(*id).unwrap();
            assert_eq!(message.text.len(), original.text.len());
            assert_eq!(message.sound, original.sound);
            changed += usize::from(message.text != original.text);
        }
    }
    assert!(changed > 0);
}
