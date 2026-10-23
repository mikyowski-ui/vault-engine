//! SPEC-007 section 4.1 (Fixture A) and FID codec domain checks.
use crate::{
    ArtCatalog, Fid, FidError, LstKind, LstTable,
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
