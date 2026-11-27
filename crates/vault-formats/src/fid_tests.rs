//! SPEC-007 section 4.1 (Fixture A) and FID codec domain checks.
use crate::{
    ArtCatalog, ArtRegistration, Fid, FidError, LstKind, LstTable,
    fid::{critter_suffix, head_suffix},
};
use std::collections::BTreeSet;

fn catalog() -> ArtCatalog {
    let mut catalog = ArtCatalog::default();
    catalog.insert(
        1,
        LstTable::parse(b"ranger,1\nmutant,0\nslimex,1,999\n", LstKind::Art).unwrap(),
    );
    catalog.insert(8, LstTable::parse(b"talker,1,2,3\n", LstKind::Art).unwrap());
    catalog.insert(0, LstTable::parse(b"knife.frm\n", LstKind::Art).unwrap());
    catalog
}

#[test]
fn spec007_fixture_a_resolves_critter_filenames() {
    let catalog = catalog();
    for (fid, expected) in [
        (0x01000000, "rangeraa.frm"),
        (0x01005000, "rangerha.frm"),
        (0x01015000, "rangerhb.frm"),
        (0x010D5000, "rangerhe.frm"),
        (0x01121000, "rangerdm.frm"),
        (0x01124000, "rangergm.frm"),
        (0x01122000, "rangeras.frm"),
        (0x01241000, "rangerch.frm"),
        (0x012D5000, "rangerhj.frm"),
        (0x011B5000, "mutantbh.frm"),
        (0x011B5002, "mutantbh.frm"),
        (0x01400000, "mutantna.frm"),
        (0x013D0000, "rangerrn.frm"),
        (0x31145000, "rangerba.fr2"),
        (0x61155000, "rangerbb.fr5"),
        (0x311B5000, "mutantbh.fr2"),
        (0x31215000, "mutantbn.fr2"),
        (0x71145000, "rangerba.fr6"),
        // Row 1 aliases row 0; the substitution happens once, without recursion.
        (0x011B5001, "rangerbh.frm"),
    ] {
        assert_eq!(
            catalog.filename(Fid(fid)).as_deref(),
            Ok(expected),
            "{fid:#010x}"
        );
    }
    assert_eq!(
        catalog.filename(Fid(0x012D0000)),
        Err(FidError::WeaponRequired(45))
    );
    assert_eq!(catalog.filename(Fid(0x0100B000)), Err(FidError::Weapon(11)));
    assert_eq!(
        catalog.path(Fid(0x01000000)).as_deref(),
        Ok("art/critters/rangeraa.frm")
    );
    // Bit 31 is ignored by filename decoding.
    assert_eq!(
        catalog.filename(Fid(0x81005000)).as_deref(),
        Ok("rangerha.frm")
    );
}

#[test]
fn spec007_fixture_a_big_endian_words_select_filenames() {
    let catalog = catalog();
    let bytes = [
        0x01, 0x2D, 0x50, 0x00, 0x31, 0x14, 0x50, 0x00, 0x01, 0x40, 0x00, 0x00,
    ];
    let names: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| catalog.filename(Fid(u32::from_be_bytes(*word))).unwrap())
        .collect();
    assert_eq!(names, ["rangerhj.frm", "rangerba.fr2", "mutantna.frm"]);
}

#[test]
fn spec007_fixture_a_constructor_probes_requested_then_fr0() {
    let catalog = catalog();
    let selector = Fid::selector_for_orientation(2);
    assert_eq!(selector, 3);
    for (animation, existing, fid, resolved) in [
        (20, &["rangerba.fr2"][..], 0x31145000, "rangerba.fr2"),
        (20, &["rangerba.fr0"][..], 0x11145000, "rangerba.fr0"),
        (20, &["rangerba.frm"][..], 0x01145000, "rangerba.frm"),
        (20, &[][..], 0x01145000, "rangerba.frm"),
        (20, &["rangerba.fr5"][..], 0x01145000, "rangerba.frm"),
        (27, &["mutantbh.fr2"][..], 0x311B5000, "mutantbh.fr2"),
        (
            33,
            &["mutantbn.fr2", "mutantbn.frm"][..],
            0x01215000,
            "mutantbn.frm",
        ),
        (45, &["rangerhj.fr2"][..], 0x012D5000, "rangerhj.frm"),
    ] {
        let existing: BTreeSet<String> = existing
            .iter()
            .map(|name| format!("art/critters/{name}"))
            .collect();
        let mut probes = Vec::new();
        let constructed = catalog.construct(1, 0, animation, 5, selector, |path| {
            probes.push(path.to_owned());
            existing.contains(path)
        });
        assert_eq!(constructed, Fid(fid), "animation {animation} {existing:?}");
        assert_eq!(catalog.filename(constructed).as_deref(), Ok(resolved));
        if animation == 33 || animation == 45 {
            assert!(probes.is_empty(), "no directional probing for {animation}");
        }
    }
    // Requested selector 1 that is missing encodes 0 without a second probe.
    let mut probes = 0;
    assert_eq!(
        catalog.construct(1, 0, 20, 5, 1, |_| {
            probes += 1;
            false
        }),
        Fid(0x01145000)
    );
    assert_eq!(probes, 1);
    // Eligible death animation requested at selector 0 can fall back to .fr0.
    assert_eq!(
        catalog.construct(1, 0, 20, 5, 0, |path| path.ends_with(".fr0")),
        Fid(0x11145000)
    );
    // Non-critter construction clears the selector without probing.
    assert_eq!(
        catalog.construct(0, 0, 20, 5, 3, |_| panic!("no probe")),
        Fid(0x00145000)
    );
}

#[test]
fn spec007_fixture_a_head_overlay_and_verbatim_categories() {
    let catalog = catalog();
    assert_eq!(
        catalog.filename(Fid(0x08014000)).as_deref(),
        Ok("talkergf4.frm")
    );
    assert_eq!(
        catalog.path(Fid(0x080A0000)).as_deref(),
        Ok("art/heads/talkernp.frm")
    );
    assert_eq!(
        catalog.filename(Fid(0x080C0000)),
        Err(FidError::HeadAnimation(12))
    );
    // Item art ignores animation/weapon/selector bits for ordinary lookup.
    assert_eq!(
        catalog.path(Fid(0x70145000)).as_deref(),
        Ok("art/items/knife.frm")
    );
    assert_eq!(
        catalog.filename(Fid(0x0B000000)),
        Err(FidError::Category(11))
    );
    assert_eq!(
        catalog.filename(Fid(0x02000000)),
        Err(FidError::MissingList(2))
    );
    assert_eq!(
        catalog.filename(Fid(0x01000003)),
        Err(FidError::MissingEntry {
            category: 1,
            index: 3
        })
    );
}

#[test]
fn spec007_fid_packing_and_suffix_domains() {
    assert_eq!(Fid::new(1, 0, 20, 5, 3), Fid(0x31145000));
    assert_eq!(Fid::new(1, 0x1fff, 0, 0x15, 9), Fid(0x11005fff));
    let fid = Fid(0x7FFF_FFFF);
    assert_eq!(
        (
            fid.base(),
            fid.weapon(),
            fid.animation(),
            fid.category(),
            fid.selector()
        ),
        (0xfff, 15, 0xff, 15, 7)
    );
    for weapon in 11..=15 {
        for animation in [0, 20, 36, 48] {
            assert_eq!(
                critter_suffix(animation, weapon),
                Err(FidError::Weapon(weapon))
            );
        }
    }
    for animation in 65..=255 {
        assert_eq!(
            critter_suffix(animation, 0),
            Err(FidError::Animation(animation))
        );
    }
    let death: Vec<_> = (20..=35)
        .map(|animation| critter_suffix(animation, 3).unwrap())
        .collect();
    assert_eq!(death.first(), Some(b"ba"));
    assert_eq!(death.last(), Some(b"bp"));
    assert_eq!(critter_suffix(47, 10), Ok(*b"ml"));
    assert_eq!(critter_suffix(38, 1), Ok(*b"dc"));
    assert_eq!(critter_suffix(63, 0), Ok(*b"rp"));
    assert_eq!(critter_suffix(37, 0), Ok(*b"cj"));
    assert_eq!(critter_suffix(14, 0), Ok(*b"ao"));
    assert_eq!(critter_suffix(17, 0), Ok(*b"ar"));
    assert_eq!(critter_suffix(19, 1), Ok(*b"at"));
    assert_eq!(critter_suffix(12, 7), Ok(*b"am"));
    assert_eq!(head_suffix(7, 12).as_deref(), Ok("bf12"));
    assert_eq!(head_suffix(0, 9).as_deref(), Ok("gv"));
}

#[test]
fn spec007_critter_rows_follow_list_token_rules() {
    let mut catalog = ArtCatalog::default();
    // Row 0: CRLF translated, leading delimiter gives an empty stem, and alias
    // -4092 masks to 12-bit index 4. Rows 1-2: a 250-byte line consumes two
    // 199-byte-bounded indexes. Row 3: 12-byte truncation and no comma field.
    // Row 4: no comma, so aliasing uses the list's default index.
    let mut bytes = b",-4092\r\n".to_vec();
    bytes.extend_from_slice(&[b'x'; 250]);
    bytes.extend_from_slice(b"\nabcdefghijklmnop;7\nplain\n");
    catalog.insert(1, LstTable::parse(&bytes, LstKind::Art).unwrap());
    catalog.default_alias = 3;
    assert_eq!(catalog.filename(Fid(0x01000000)).as_deref(), Ok("aa.frm"));
    assert_eq!(
        catalog.filename(Fid(0x011B0000)).as_deref(),
        Ok("plainbh.frm")
    );
    assert_eq!(
        catalog.filename(Fid(0x01000001)).as_deref(),
        Ok("xxxxxxxxxxxxaa.frm")
    );
    assert_eq!(
        catalog.filename(Fid(0x01000002)).as_deref(),
        Ok("xxxxxxxxxxxxaa.frm")
    );
    assert_eq!(
        catalog.filename(Fid(0x01000003)).as_deref(),
        Ok("abcdefghijklaa.frm")
    );
    assert_eq!(
        catalog.filename(Fid(0x011B0004)).as_deref(),
        Ok("abcdefghijklbh.frm")
    );
    assert_eq!(
        catalog.filename(Fid(0x01000005)),
        Err(FidError::MissingEntry {
            category: 1,
            index: 5
        })
    );
}

#[test]
fn spec005_jumpsuit_rows_match_base_names_without_case_and_the_last_wins() {
    let mut catalog = ArtCatalog::default();
    catalog.insert(
        1,
        LstTable::parse(
            b"reserv,0\nHFJMPS,4\nhmjmps;old\nhfjmps\thalf\nhmjmps 11\n",
            LstKind::Art,
        )
        .unwrap(),
    );
    assert_eq!(catalog.last_row(1, b"hfjmps"), Some(3));
    assert_eq!(catalog.last_row(1, b"hmjmps"), Some(4));
    assert_eq!(catalog.last_row(1, b"hapowr"), None);
    assert_eq!(catalog.last_row(0, b"hmjmps"), None, "no list");
}

#[test]
fn spec014_called_shot_picture_and_labels_follow_the_critter_alias() {
    // SPEC-014 §3.38: the `na` picture (code 64) and the label messages use
    // the row's alias; row 0 has no alias field and takes the default 0.
    let mut catalog = ArtCatalog::default();
    catalog.insert(
        1,
        LstTable::parse(b"reserv\nhmjmps,1\nhmmaxx,1\n", LstKind::Art).unwrap(),
    );
    assert_eq!(catalog.critter_alias(0), Some(0));
    assert_eq!(catalog.critter_alias(2), Some(1));
    assert_eq!(catalog.critter_alias(3), None);
    assert_eq!(
        catalog.filename(Fid::new(1, 2, 64, 0, 0)).as_deref(),
        Ok("hmjmpsna.frm")
    );
}

// `docs/extended-art-namespaces.md`: explicit wrap, extended identifiers and
// registered rows. Every list here is synthetic, generated as text.

/// `rows` art list lines `<prefix>0000.frm` onward, one per line.
fn generated(prefix: &str, rows: u32) -> LstTable {
    let text: String = (0..rows)
        .map(|row| format!("{prefix}{row:04}.frm\n"))
        .collect();
    LstTable::parse(text.as_bytes(), LstKind::Art).unwrap()
}

/// Vector A: a 4,100-row scenery list.
fn vector_a() -> ArtCatalog {
    let mut catalog = ArtCatalog::default();
    catalog.insert(2, generated("s", 4100));
    catalog
}

fn art(category: u8, file: &str, index: u32) -> ArtRegistration {
    ArtRegistration {
        category,
        file: file.into(),
        index,
    }
}

#[test]
fn extended_vector_a_resolves_plain_and_extended_rows() {
    let catalog = vector_a();
    assert_eq!(catalog.list_len(2), 4100);
    for (row, plain, extended, resolves) in [
        (0, Some(0x0200_0000), 0x8200_0000, Some("s0000.frm")),
        (5, Some(0x0200_0005), 0x8200_0005, Some("s0005.frm")),
        (4095, Some(0x0200_0FFF), 0x8200_0FFF, Some("s4095.frm")),
        (4096, None, 0x8200_1000, Some("s4096.frm")),
        (4099, None, 0x8200_1003, Some("s4099.frm")),
        (4100, None, 0x8200_1004, None),
    ] {
        assert_eq!(Fid::extended(2, row), Ok(Fid(extended)), "row {row}");
        assert_eq!(
            Fid::try_new(2, row, 0, 0, 0).ok(),
            plain.map(Fid),
            "row {row}"
        );
        assert!(catalog.is_extended(Fid(extended)), "row {row}");
        assert_eq!(Fid(extended).row(&catalog), Some(row));
        match resolves {
            Some(name) => {
                assert_eq!(catalog.filename(Fid(extended)).as_deref(), Ok(name));
                assert_eq!(
                    catalog.path(Fid(extended)),
                    Ok(format!("art/scenery/{name}"))
                );
                assert_eq!(catalog.row_path(2, row), Ok(format!("art/scenery/{name}")));
                assert_eq!(catalog.row_name(2, row), Some(name));
                if let Some(plain) = plain {
                    assert_eq!(catalog.filename(Fid(plain)).as_deref(), Ok(name));
                    assert_eq!(catalog.canonical_fid(2, row), Some(Fid(plain)));
                } else {
                    assert_eq!(catalog.canonical_fid(2, row), Some(Fid(extended)));
                }
            }
            None => {
                assert_eq!(
                    catalog.filename(Fid(extended)),
                    Err(FidError::MissingRow {
                        category: 2,
                        row: 4100
                    })
                );
                assert_eq!(
                    catalog.row_path(2, row),
                    Err(FidError::MissingRow {
                        category: 2,
                        row: 4100
                    })
                );
                assert_eq!(catalog.row_name(2, row), None);
                assert_eq!(catalog.canonical_fid(2, row), None);
            }
        }
    }
    assert_eq!(
        Fid::try_new(2, 4096, 0, 0, 0),
        Err(FidError::BaseRange {
            category: 2,
            index: 4096
        })
    );
    assert_eq!(Fid::new(2, 4096, 0, 0, 0), Fid(0x0200_0000));
    // Bits 12-15 with bit 31 clear: row 0, as today.
    assert!(!catalog.is_extended(Fid(0x0200_1000)));
    assert_eq!(Fid(0x0200_1000).row(&catalog), Some(0));
    assert_eq!(
        catalog.filename(Fid(0x0200_1000)).as_deref(),
        Ok("s0000.frm")
    );
    let [line] = catalog.diagnostics() else {
        panic!("one diagnostic: {:?}", catalog.diagnostics());
    };
    for needle in [
        "art/scenery/scenery.lst",
        "4,100 rows",
        "first unreachable row 4096",
        "extended identifiers",
    ] {
        assert!(line.contains(needle), "{needle}: {line}");
    }
}

#[test]
fn extended_vector_b_a_list_of_exactly_4096_rows_ignores_bit_31() {
    let mut catalog = ArtCatalog::default();
    catalog.insert(2, generated("s", 4096));
    assert!(catalog.diagnostics().is_empty());
    assert!(!catalog.is_extended(Fid(0x8200_1000)));
    assert_eq!(Fid(0x8200_1000).row(&catalog), Some(0));
    assert_eq!(
        catalog.filename(Fid(0x8200_1000)).as_deref(),
        Ok("s0000.frm")
    );
    assert_eq!(
        catalog.filename(Fid(0x8200_0005)).as_deref(),
        Ok("s0005.frm")
    );
    // No row past 4095 exists, so none has a canonical identifier.
    assert_eq!(catalog.canonical_fid(2, 4096), None);
    assert_eq!(catalog.canonical_fid(2, 4095), Some(Fid(0x0200_0FFF)));
}

#[test]
fn extended_edge_cases_4097_rows_short_lists_and_capped_categories() {
    // 4,097 rows: row 4096 is reachable only as an extended FID.
    let mut catalog = ArtCatalog::default();
    catalog.insert(5, generated("m", 4097));
    assert_eq!(catalog.diagnostics().len(), 1);
    assert!(catalog.diagnostics()[0].contains("art/misc/misc.lst: 4,097 rows"));
    assert_eq!(
        catalog.filename(Fid(0x8500_1000)).as_deref(),
        Ok("m4096.frm")
    );
    for plain in [0x0500_0000, 0x0500_1000, 0x05FF_F000, 0x7500_0000] {
        assert_eq!(
            catalog.filename(Fid(plain)).as_deref(),
            Ok("m0000.frm"),
            "{plain:#010x}"
        );
    }
    // Bit 31 under a short list keeps today's meaning: bit ignored.
    let short = catalog_with(0, b"knife.frm\nspear.frm\n");
    assert!(!short.is_extended(Fid(0x8000_1001)));
    assert_eq!(short.filename(Fid(0x8000_1001)).as_deref(), Ok("spear.frm"));
    assert_eq!(short.filename(Fid(0x8000_0001)).as_deref(), Ok("spear.frm"));
    assert_eq!(Fid(0x8000_1001).row(&short), Some(1));
    assert!(short.diagnostics().is_empty());
    // Critters and heads stay capped: bit 31 never counts, every row from
    // 4096 is unreachable, and the diagnostic says so.
    let mut capped = ArtCatalog::default();
    let critters: String = (0..4100).map(|row| format!("c{row:04}\n")).collect();
    capped.insert(
        1,
        LstTable::parse(critters.as_bytes(), LstKind::Art).unwrap(),
    );
    capped.insert(8, generated("h", 4097));
    assert!(!capped.is_extended(Fid(0x8100_1000)));
    assert_eq!(Fid(0x8100_1000).row(&capped), Some(0));
    assert_eq!(
        capped.filename(Fid(0x8100_1000)).as_deref(),
        Ok("c0000da.frm"),
        "weapon code 1 as today"
    );
    assert_eq!(capped.canonical_fid(1, 4096), None);
    assert_eq!(capped.canonical_fid(1, 7), Some(Fid(0x0100_0007)));
    assert_eq!(capped.row_path(1, 0), Err(FidError::RowCategory(1)));
    assert_eq!(capped.diagnostics().len(), 2);
    assert!(capped.diagnostics()[0].starts_with("art/critters/critters.lst: 4,100 rows"));
    assert!(capped.diagnostics()[1].starts_with("art/heads/heads.lst: 4,097 rows"));
    for line in capped.diagnostics() {
        assert!(line.contains("first unreachable row 4096"), "{line}");
        assert!(line.contains("never drawn"), "{line}");
    }
    // 0xFFFFFFFF (category 15) keeps its meaning: no directory.
    assert!(!vector_a().is_extended(Fid(u32::MAX)));
    assert_eq!(Fid(u32::MAX).row(&vector_a()), None);
    assert_eq!(
        vector_a().filename(Fid(u32::MAX)),
        Err(FidError::Category(15))
    );
}

fn catalog_with(category: u8, text: &[u8]) -> ArtCatalog {
    let mut catalog = ArtCatalog::default();
    catalog.insert(category, LstTable::parse(text, LstKind::Art).unwrap());
    catalog
}

#[test]
fn extended_constructors_refuse_what_does_not_fit() {
    assert_eq!(Fid::try_new(1, 0, 20, 5, 3), Ok(Fid::new(1, 0, 20, 5, 3)));
    assert_eq!(Fid::try_new(10, 4095, 0, 0, 0), Ok(Fid(0x0A00_0FFF)));
    assert_eq!(
        Fid::try_new(11, 0, 0, 0, 0),
        Err(FidError::CategoryRange(11))
    );
    assert_eq!(
        Fid::try_new(16, 0, 0, 0, 0),
        Err(FidError::CategoryRange(16))
    );
    assert_eq!(
        Fid::try_new(0, u32::MAX, 0, 0, 0),
        Err(FidError::BaseRange {
            category: 0,
            index: u32::MAX
        })
    );
    assert_eq!(Fid::extended(1, 5), Err(FidError::RowCategory(1)));
    assert_eq!(Fid::extended(8, 5), Err(FidError::RowCategory(8)));
    assert_eq!(Fid::extended(11, 5), Err(FidError::CategoryRange(11)));
    assert_eq!(
        Fid::extended(4, 0x0100_0000),
        Err(FidError::BaseRange {
            category: 4,
            index: 0x0100_0000
        })
    );
    assert_eq!(Fid::extended(10, 0x00ff_ffff), Ok(Fid(0x8AFF_FFFF)));
    assert_eq!(
        FidError::BaseRange {
            category: 2,
            index: 4096
        }
        .to_string(),
        "art base index 4096 does not fit category 2's identifier"
    );
    assert_eq!(
        FidError::CategoryRange(12).to_string(),
        "art category 12 is outside 0..=10"
    );
}

#[test]
fn extended_vector_c_registered_rows_extend_the_resolved_list() {
    let mut catalog = catalog_with(0, b"knife.frm\nspear.frm\nrock.frm\n");
    catalog.register(&[art(0, "tc_a.frm", 4100)]).unwrap();
    assert_eq!(catalog.list_len(0), 4101);
    for row in 3..4100 {
        assert_eq!(catalog.row_name(0, row), None, "gap row {row}");
    }
    assert_eq!(catalog.row_name(0, 2), Some("rock.frm"));
    assert_eq!(catalog.row_name(0, 4100), Some("tc_a.frm"));
    assert_eq!(catalog.row_name(0, 4101), None);
    assert_eq!(catalog.canonical_fid(0, 4100), Some(Fid(0x8000_1004)));
    assert_eq!(catalog.canonical_fid(0, 4101), None);
    assert_eq!(
        catalog.path(Fid(0x8000_1004)).as_deref(),
        Ok("art/items/tc_a.frm")
    );
    assert_eq!(
        catalog.row_path(0, 4100).as_deref(),
        Ok("art/items/tc_a.frm")
    );
    assert_eq!(catalog.row_token(0, 4100), Some(&b"tc_a.frm"[..]));
    assert_eq!(catalog.last_row_wide(0, b"TC_A.FRM"), Some(4100));
    assert_eq!(catalog.last_row(0, b"tc_a.frm"), Some(4100));
    // Gap rows have no filename, plain or extended.
    assert_eq!(
        catalog.filename(Fid(0x8000_0005)),
        Err(FidError::MissingRow {
            category: 0,
            row: 5
        })
    );
    assert_eq!(
        catalog.filename(Fid(0x0000_0005)),
        Err(FidError::MissingEntry {
            category: 0,
            index: 5
        })
    );
    // Rows of the file keep both forms.
    assert_eq!(
        catalog.filename(Fid(0x8000_0001)).as_deref(),
        Ok("spear.frm")
    );
    assert_eq!(
        catalog.filename(Fid(0x0000_0001)).as_deref(),
        Ok("spear.frm")
    );
    let [line] = catalog.diagnostics() else {
        panic!("{:?}", catalog.diagnostics());
    };
    assert!(
        line.starts_with("art/items/items.lst: 4,101 rows (3 from the file"),
        "{line}"
    );
}

#[test]
fn extended_registered_rows_below_4096_answer_plain_identifiers() {
    let mut catalog = catalog_with(0, b"knife.frm\n");
    catalog.insert(1, LstTable::parse(b"ranger,1\n", LstKind::Art).unwrap());
    catalog
        .register(&[art(0, "tc_b.frm", 10), art(1, "tcmutn", 4)])
        .unwrap();
    assert!(catalog.diagnostics().is_empty());
    assert_eq!(catalog.list_len(0), 11);
    assert_eq!(
        catalog.filename(Fid(0x0000_000A)).as_deref(),
        Ok("tc_b.frm")
    );
    assert_eq!(catalog.canonical_fid(0, 10), Some(Fid(0x0000_000A)));
    // A registered critter row takes suffixes and the default alias.
    assert_eq!(
        catalog.filename(Fid::new(1, 4, 0, 0, 0)).as_deref(),
        Ok("tcmutnaa.frm")
    );
    assert_eq!(
        catalog.filename(Fid::new(1, 4, 20, 5, 3)).as_deref(),
        Ok("tcmutnba.fr2")
    );
    assert_eq!(catalog.critter_alias(4), Some(0));
    assert_eq!(catalog.base_token(1, 4), Some(&b"tcmutn"[..]));
    assert_eq!(catalog.last_row(1, b"tcmutn"), Some(4));
    assert_eq!(
        catalog.filename(Fid::new(1, 2, 0, 0, 0)),
        Err(FidError::MissingEntry {
            category: 1,
            index: 2
        })
    );
    // A category with no list file holds only its registered rows.
    catalog.register(&[art(6, "tc_icon.frm", 1)]).unwrap();
    assert_eq!(catalog.list_len(6), 2);
    assert_eq!(
        catalog.filename(Fid(0x0600_0001)).as_deref(),
        Ok("tc_icon.frm")
    );
    assert_eq!(
        catalog.filename(Fid(0x0600_0000)),
        Err(FidError::MissingEntry {
            category: 6,
            index: 0
        })
    );
    assert_eq!(catalog.list_len(7), 0);
    assert_eq!(
        catalog.filename(Fid(0x0700_0000)),
        Err(FidError::MissingList(7))
    );
}

#[test]
fn extended_vector_d_register_refusals_name_category_and_index() {
    let base = || catalog_with(2, b"a.frm\nb.frm\nc.frm\n");
    for (rows, needles) in [
        (
            vec![art(2, "x.frm", 2)],
            &["row 2", "scenery", "3 rows"][..],
        ),
        (
            vec![art(1, "crit", 4096)],
            &["row 4096", "critters", "4095"],
        ),
        (vec![art(8, "head", 5000)], &["row 5000", "heads"]),
        (vec![art(11, "x.frm", 9)], &["category 11", "row 9"]),
        (
            vec![art(2, "x.frm", 0x0100_0000)],
            &["row 16777216", "scenery"],
        ),
        (
            vec![art(2, "x.frm", 5000), art(2, "y.frm", 5000)],
            &["row 5000", "scenery", "twice"],
        ),
        (vec![art(2, "two words", 9)], &["row 9", "list name"]),
        (vec![art(2, "", 9)], &["row 9", "list name"]),
        (vec![art(2, "thirteen.frmx", 9)], &["list name"]),
    ] {
        let mut catalog = base();
        let error = catalog.register(&rows).unwrap_err();
        for needle in needles {
            assert!(error.contains(needle), "{needle}: {error}");
        }
        // Nothing is registered after a refusal.
        assert_eq!(catalog.list_len(2), 3, "{error}");
        assert_eq!(catalog.list_len(1), 0, "{error}");
    }
    // An index at the list's length is the first free row; registering it
    // twice across calls is refused.
    let mut catalog = base();
    catalog.register(&[art(2, "d.frm", 3)]).unwrap();
    assert_eq!(catalog.row_name(2, 3), Some("d.frm"));
    let error = catalog.register(&[art(2, "e.frm", 3)]).unwrap_err();
    assert!(
        error.contains("row 3") && error.contains("twice"),
        "{error}"
    );
    // Critter and head row 4095 is the last allowed.
    catalog.register(&[art(1, "crit", 4095)]).unwrap();
    assert_eq!(catalog.base_token(1, 4095), Some(&b"crit"[..]));
}

/// §5.4: `Fid::extended(c, r)?.row(catalog) == Some(r)` for every row the
/// list holds, and the canonical FID of a row below 4096 is the plain one.
#[test]
fn extended_round_trip_over_a_spread_of_rows() {
    let mut catalog = ArtCatalog::default();
    for category in [0, 2, 3, 4, 5, 6, 7, 9, 10] {
        catalog.insert(category, generated("r", 4100));
        catalog
            .register(&[art(category, "far.frm", 70_000)])
            .unwrap();
    }
    for category in [0, 2, 3, 4, 5, 6, 7, 9, 10] {
        assert_eq!(catalog.list_len(category), 70_001);
        for row in [
            0, 1, 2, 4094, 4095, 4096, 4097, 4099, 8191, 8192, 65535, 65536, 69_999, 70_000,
        ] {
            let extended = Fid::extended(category, row).unwrap();
            assert_eq!(extended.row(&catalog), Some(row), "{category} {row}");
            let canonical = catalog.canonical_fid(category, row).unwrap();
            assert_eq!(canonical.row(&catalog), Some(row), "{category} {row}");
            if row < 4096 {
                assert_eq!(canonical, Fid::try_new(category, row, 0, 0, 0).unwrap());
                assert_eq!(catalog.filename(canonical), catalog.filename(extended));
            } else {
                assert_eq!(canonical, extended);
            }
        }
        assert_eq!(catalog.canonical_fid(category, 70_001), None);
        assert_eq!(
            catalog
                .filename(Fid::extended(category, 70_000).unwrap())
                .as_deref(),
            Ok("far.frm")
        );
    }
    assert_eq!(catalog.diagnostics().len(), 9);
}

#[test]
fn extended_catalog_load_records_the_diagnostic() {
    let mut scenery: String = (0..4100).map(|row| format!("s{row:04}.frm\n")).collect();
    scenery.push_str("tail.frm\n");
    let dir = crate::MemoryDir::new([
        (
            "art/scenery/scenery.lst",
            crate::ByteSource::new(scenery.into_bytes()).unwrap(),
        ),
        (
            "art/items/items.lst",
            crate::ByteSource::new(b"knife.frm\n".to_vec()).unwrap(),
        ),
    ])
    .unwrap();
    let catalog = ArtCatalog::load(&dir).unwrap();
    assert_eq!(catalog.list_len(2), 4101);
    assert_eq!(catalog.list_len(0), 1);
    assert_eq!(
        catalog.diagnostics(),
        [
            "art/scenery/scenery.lst: 4,101 rows; first unreachable row 4096: plain identifiers \
          address rows 0-4095, so rows from 4096 need extended identifiers"
        ]
    );
    assert_eq!(
        catalog.filename(Fid(0x8200_1004)).as_deref(),
        Ok("tail.frm")
    );
}
