use crate::*;
use crate::{
    frm::{FrameCursor, Rect},
    palette::{CYCLE_RANGES, PaletteCycler, display_channel},
};

use crate::fixture_assets as fixtures;

#[test]
fn spec001_palette_values_transparency_and_tables() {
    let bytes = fixtures::palette();
    assert_eq!(bytes.len(), 0x8304);
    let palette = Palette::parse(&bytes).unwrap();
    assert_eq!(
        &palette.rgba8()[..4],
        &[
            [4, 8, 12, 255],
            [252, 128, 4, 255],
            [64, 32, 16, 255],
            [0, 0, 0, 255]
        ]
    );
    assert!(palette.is_mapped(0));
    assert!(!palette.is_mapped(3));
    assert_eq!(palette.lookup_rgb5([31, 31, 31]), Some(0));
    assert_eq!(palette.lookup_rgb5([32, 0, 0]), None);
    assert_eq!(palette.intensity(3, 255), 0);
    assert!(palette.supplied_tables().is_none());
    let mut extended = bytes.clone();
    extended[0x8300..].copy_from_slice(b"CWEN");
    extended.extend(std::iter::repeat_n(42, 3 * 65536));
    let loaded = Palette::parse(&extended).unwrap();
    assert_eq!(loaded.supplied_tables().unwrap().len(), 3 * 65536);
    assert_eq!(loaded.intensity(1, 127), 42);
    for end in [0x8304, 0x18303, 0x18304, 0x28304, 0x38303] {
        assert!(Palette::parse(&extended[..end]).is_err());
    }
    extended[0x8300..0x8304].copy_from_slice(b"NEWC");
    assert!(
        Palette::parse(&extended)
            .unwrap()
            .supplied_tables()
            .is_none()
    );
}

#[test]
fn spec001_fixture_a2_shipped_base_form_and_length_boundaries() {
    // Fixture A2: Fixture A without its discriminator, the ordinary shipped
    // 0x8300-byte form; every Fixture A result is unchanged.
    let fixture_a = fixtures::palette();
    let a2 = fixtures::sparse(include_str!(
        "../../../tests/fixtures/formats/palette-base.hex"
    ));
    assert_eq!(a2, fixture_a[..0x8300]);
    let shipped = Palette::parse(&a2).unwrap();
    let explicit = Palette::parse(&fixture_a).unwrap();
    assert!(shipped.supplied_tables().is_none(), "generated tables");
    assert_eq!(shipped.rgba8(), explicit.rgba8());
    for index in 0..=255 {
        assert_eq!(shipped.is_mapped(index), explicit.is_mapped(index));
        assert_eq!(
            shipped.intensity(index, 127),
            explicit.intensity(index, 127)
        );
    }
    assert!(!shipped.is_mapped(3));
    // §4.1.1 length boundaries for the same content.
    let accepts = |bytes: &[u8]| Palette::parse(bytes).is_ok();
    assert!(!accepts(&a2[..0x82ff]), "lookup table one byte short");
    assert!(accepts(&a2), "0x8300: generated tables");
    let mut partial = a2.clone();
    partial.extend([0, 0]);
    assert!(!accepts(&partial), "0x8302: partial discriminator");
    assert!(accepts(&fixture_a), "0x8304: Fixture A");
    let mut truncated = a2;
    truncated.extend(b"CWEN");
    assert!(!accepts(&truncated), "matching discriminator, no tables");
}

#[test]
fn base_palettes_need_no_tag_but_optional_extensions_must_be_complete() {
    let bytes = fixtures::sparse(include_str!(
        "../../../tests/fixtures/formats/palette-base.hex"
    ));
    assert_eq!(bytes.len(), 33_536);
    let palette = Palette::parse(&bytes).unwrap();
    assert!(palette.supplied_tables().is_none());
    assert_eq!(palette.rgba8()[1], [252, 128, 4, 255]);
    assert_eq!(
        palette.rgba8(),
        Palette::parse(&fixtures::palette()).unwrap().rgba8()
    );
    for length in 1..4 {
        let mut partial_tag = bytes.clone();
        partial_tag.extend_from_slice(&b"CWEN"[..length]);
        assert!(Palette::parse(&partial_tag).is_err());
    }
    let mut tagged = bytes;
    tagged.extend_from_slice(b"CWEN");
    assert!(Palette::parse(&tagged).is_err());
    tagged.extend(std::iter::repeat_n(17, 3 * 65536));
    assert_eq!(Palette::parse(&tagged).unwrap().intensity(1, 127), 17);
}

#[test]
fn spec001_gamma_display_conversion() {
    // Neutral gamma is exactly x4: 0, 1, 31, 32, 63 -> 0, 4, 124, 128, 252.
    for channel in 0..=63 {
        assert_eq!(display_channel(channel, 1.0), channel * 4);
    }
    // Raise, clamp to 0..=63, truncate, then x4 (not 255 * (c/63)^gamma).
    assert_eq!(
        [0, 1, 2, 31, 32, 63].map(|channel| display_channel(channel, 1.2)),
        [0, 4, 8, 244, 252, 252]
    );
    assert_eq!(
        [0, 2, 31, 63].map(|channel| display_channel(channel, 0.5)),
        [0, 4, 20, 28]
    );
    let palette = Palette::parse(&fixtures::palette()).unwrap();
    let display = palette.display_rgba8(1.2);
    assert_eq!(display, {
        let mut expected = palette.rgba8();
        for (index, entry) in expected.iter_mut().enumerate() {
            let rgb6 = palette.rgb6(index as u8);
            *entry = [
                display_channel(rgb6[0], 1.2),
                display_channel(rgb6[1], 1.2),
                display_channel(rgb6[2], 1.2),
                255,
            ];
        }
        expected
    });
    // Unavailable entries are black at any gamma; index 0 is an opaque entry.
    assert_eq!(display[3], [0, 0, 0, 255]);
    assert_eq!(palette.display_rgba8(0.5)[0][3], 255);
}

#[test]
fn palette_cycle_fade_suspension_and_gamma_writes() {
    let mut display = [[0; 4]; 256];
    let mut cycler = PaletteCycler::default();
    cycler.gamma = 0.5;
    assert!(cycler.poll(200, &mut display));
    // Slime's first six-bit triple (0,27,0) uses the same display conversion.
    assert_eq!(display[229], [0, 20, 0, 255]);
    let previous = cycler.suspend_for_fade();
    assert!(previous && !cycler.enabled);
    let faded = display;
    assert!(!cycler.poll(10_000, &mut display));
    assert_eq!(display, faded);
    cycler.restore_after_fade(previous);
    assert!(cycler.poll(10_000, &mut display));
    // A fade that began with cycling disabled restores the disabled state.
    cycler.enabled = false;
    let previous = cycler.suspend_for_fade();
    cycler.restore_after_fade(previous);
    assert!(!cycler.enabled);
}

#[test]
fn palette_cycle_timers_rotation_pulse_disable_and_reset() {
    let mut display = Palette::parse(&fixtures::palette()).unwrap().rgba8();
    let original = display;
    let mut cycler = PaletteCycler::default();
    assert!(!cycler.poll(32, &mut display));
    assert!(cycler.poll(33, &mut display));
    assert_eq!(display[254], [16, 0, 0, 255]);
    cycler.poll(200, &mut display);
    for range in CYCLE_RANGES {
        assert_eq!(
            display[usize::from(range.first)][..3],
            range.colors6[0].map(|c| c * 4)
        );
    }
    cycler.poll(100_000, &mut display); // One update per group, not catch-up.
    assert_eq!(display[229], [40, 128, 24, 255]);
    assert_eq!(&display[..229], &original[..229]);
    assert_eq!(display[255], original[255]);
    cycler.enabled = false;
    let paused = display;
    assert!(!cycler.poll(200_000, &mut display));
    assert_eq!(display, paused);
    cycler.reset();
    cycler.interval_multiplier = 0;
    cycler.poll(0, &mut display);
    assert_eq!(display[229], [24, 120, 12, 255]); // Phase survived reset.
    let mut pulse = PaletteCycler::default();
    pulse.interval_multiplier = 0;
    for step in 1..=30 {
        pulse.poll(step, &mut display);
        assert_eq!(
            display[254][0],
            if step <= 15 {
                step as u8 * 16
            } else {
                (30 - step) as u8 * 16
            }
        );
    }
}

#[test]
fn spec001_shared_frame_and_alignment() {
    let bytes = fixtures::shared_frm();
    assert_eq!(bytes.len(), 76);
    let archive = FrmArchive::parse(&bytes).unwrap();
    assert_eq!(
        (
            archive.effective_fps(),
            archive.frame_interval_ms(),
            archive.action_frame
        ),
        (10, Some(100), 0)
    );
    assert_eq!(
        (
            archive.frame_count(),
            archive.stored_sequences(),
            archive.frame_area_len
        ),
        (1, 1, 14)
    );
    for direction in 0..6 {
        let frame = archive.frame(direction, 0).unwrap();
        assert_eq!(frame.file_offset, 62);
        assert_eq!(
            (frame.width, frame.height, frame.displacement),
            (2, 1, [0, 0])
        );
        assert_eq!(frame.pixels(), &[0, 1]);
        assert!(std::ptr::eq(frame, archive.frame(0, 0).unwrap()));
    }
    let frame = archive.frame(0, 0).unwrap();
    assert_eq!(
        frame.world_rect([100, 100], archive.shifts[0], [0, 0]),
        Rect {
            left: 101,
            top: 99,
            right: 102,
            bottom: 99
        }
    );
    assert_eq!(frame.cursor_hotspot(archive.shifts[0]), [-1, 1]);
    assert_eq!(frame.centered_origin([0, 0], [10, 9]), [4, 4]);
    assert!(archive.frame(6, 0).is_none());
    assert!(archive.frame(0, 1).is_none());
}

#[test]
fn spec001_animated_blocks_and_displacement() {
    let bytes = fixtures::animated_frm();
    assert_eq!(bytes.len(), 116);
    let archive = FrmArchive::parse(&bytes).unwrap();
    assert_eq!(
        (
            archive.effective_fps(),
            archive.frame_interval_ms(),
            archive.action_frame
        ),
        (12, Some(83), 1)
    );
    assert_eq!(archive.offsets, [0, 0, 28, 28, 28, 28]);
    assert_eq!(archive.stored_sequences(), 2);
    for direction in 0..6 {
        let (offsets, pixels): ([usize; 2], [&[u8]; 2]) = if direction < 2 {
            ([0x3e, 0x4c], [&[1, 2], &[0, 3]])
        } else {
            ([0x5a, 0x67], [&[4], &[5]])
        };
        for frame in 0..2 {
            let loaded = archive.frame(direction, frame).unwrap();
            assert_eq!(loaded.file_offset, offsets[frame]);
            assert_eq!(loaded.pixels(), pixels[frame]);
        }
    }
    let first = archive.accumulated_displacement(0, 0).unwrap();
    let second = archive.accumulated_displacement(0, 1).unwrap();
    assert_eq!(first, [1, -1]);
    assert_eq!(second, [3, 0]);
    // A fresh isolated forward pass through the engine cursor, then reversal.
    let mut cursor = FrameCursor::default();
    assert!(archive.advance(&mut cursor, 0));
    assert_eq!(
        cursor,
        FrameCursor {
            frame: Some(0),
            displacement: first
        }
    );
    let frame0_rect = Rect {
        left: 100,
        top: 99,
        right: 101,
        bottom: 99,
    };
    assert_eq!(
        archive
            .frame(0, 0)
            .unwrap()
            .world_rect([100, 100], archive.shifts[0], cursor.displacement),
        frame0_rect
    );
    assert!(archive.advance(&mut cursor, 0));
    assert_eq!(cursor.displacement, second);
    assert!(!archive.advance(&mut cursor, 0), "no frame past the count");
    assert_eq!(cursor.displacement, second);
    // Reversing from frame 1 subtracts (2,1), restoring frame 0's rectangle.
    assert!(archive.reverse(&mut cursor, 0));
    assert_eq!(
        cursor,
        FrameCursor {
            frame: Some(0),
            displacement: first
        }
    );
    assert_eq!(
        archive
            .frame(0, 0)
            .unwrap()
            .world_rect([100, 100], archive.shifts[0], cursor.displacement),
        frame0_rect
    );
    assert!(archive.reverse(&mut cursor, 0));
    assert_eq!(cursor, FrameCursor::default());
    assert!(!archive.reverse(&mut cursor, 0));
    // Selecting a frame record alone does not accumulate displacement.
    assert!(archive.frame(0, 1).is_some());
    assert_eq!(cursor.displacement, [0, 0]);
    assert_eq!(
        archive
            .frame(0, 1)
            .unwrap()
            .world_rect([100, 100], archive.shifts[0], second),
        Rect {
            left: 103,
            top: 99,
            right: 103,
            bottom: 100
        }
    );
    assert_eq!(
        archive
            .frame(2, 0)
            .unwrap()
            .world_rect([100, 100], archive.shifts[2], [0, 0]),
        Rect {
            left: 98,
            top: 103,
            right: 98,
            bottom: 103
        }
    );
    assert_eq!(
        archive
            .frame(3, 0)
            .unwrap()
            .world_rect([100, 100], archive.shifts[3], [0, 0]),
        Rect {
            left: 100,
            top: 100,
            right: 100,
            bottom: 100
        }
    );
}

#[test]
fn frm_rejects_unsafe_offsets_sizes_counts_and_truncation_but_retains_signed_timing() {
    let original = fixtures::animated_frm();
    for (offset, patch) in [
        (8, vec![0, 0]),
        (8, vec![0xff, 0xff]),
        (0x22, vec![0, 0, 0, 1]),
        (0x2a, vec![0, 0, 0, 29]),
        (0x2e, vec![0, 0, 0, 0]), // nonadjacent reuse
        (0x3e, vec![0xff, 0xff]),
        (0x40, vec![0xff, 0xff]),
        (0x42, vec![0x7f, 0xff, 0xff, 0xff]),
        (0x42, vec![0, 0, 0, 1]),
        (0x22, vec![0xff; 4]),
    ] {
        let mut mutated = original.clone();
        mutated[offset..offset + patch.len()].copy_from_slice(&patch);
        assert!(FrmArchive::parse(&mutated).is_err(), "offset {offset:#x}");
    }
    let mut bytes = original;
    bytes[..4].copy_from_slice(&123_u32.to_be_bytes());
    bytes[4..8].copy_from_slice(&[0xff, 0xff, 0xff, 0xfe]);
    let parsed = FrmArchive::parse(&bytes).unwrap();
    assert_eq!(
        (parsed.version, parsed.effective_fps(), parsed.action_frame),
        (123, -1, -2)
    );
    assert_eq!(parsed.frame_interval_ms(), None);
}

#[test]
fn split_animations_count_one_stored_sequence_and_ignore_the_unsplit_size_excess() {
    let bytes = fixtures::sparse(include_str!(
        "../../../tests/fixtures/formats/split.frm.hex"
    ));
    let archive = FrmArchive::parse(&bytes).unwrap();
    assert_eq!(archive.frame_count(), 2);
    assert_eq!(archive.stored_sequences(), 1);
    assert_eq!(archive.frame_area_len, 150);
    assert_eq!(archive.stored_frame_area_len(), 28);
    for direction in 0..6 {
        let path = VfsPath::new(&format!("art/critters/synthetic.fr{direction}")).unwrap();
        validate_asset(&path, &bytes).unwrap().unwrap();
        assert!(std::ptr::eq(
            archive.frame(direction, 0).unwrap(),
            archive.frame(0, 0).unwrap()
        ));
        assert_eq!(archive.frame(direction, 1).unwrap().file_offset, 0x4c);
        assert_eq!(archive.frame(direction, 1).unwrap().pixels(), &[0, 3]);
    }
    assert_ne!(archive.shifts[0], archive.shifts[3]);
    for end in 0..bytes.len() {
        assert!(
            FrmArchive::parse(&bytes[..end]).is_err(),
            "accepted truncated split frame at {end}"
        );
    }
    // The declared total is metadata only: no total is enforced.
    for area in [27, 28, -1] {
        let mut declared = bytes.clone();
        declared[0x3a..0x3e].copy_from_slice(&i32::to_be_bytes(area));
        assert_eq!(FrmArchive::parse(&declared).unwrap().frame_area_len, area);
    }
    let mut multi = fixtures::animated_frm();
    multi[0x3a..0x3e].copy_from_slice(&55_u32.to_be_bytes());
    let multi = FrmArchive::parse(&multi).unwrap();
    assert_eq!(
        (multi.frame_area_len, multi.stored_frame_area_len()),
        (55, 54)
    );
}

#[test]
fn frm_safe_rule_accepts_empty_and_oversized_payloads() {
    // One frame per direction: a 2 x 1 frame with a 3-byte payload, then the
    // next frame starts after the declared count; a 0 x 0 frame is valid.
    let mut bytes = Vec::new();
    for word in [4_u32.to_be_bytes(), [0, 10, 0, 0]] {
        bytes.extend(word);
    }
    bytes.extend(2_i16.to_be_bytes());
    bytes.extend([0; 24 + 24]);
    bytes.extend(28_i32.to_be_bytes());
    for (width, height, payload) in [(2_i16, 1_i16, &[5_u8, 6, 7][..]), (0, 0, &[][..])] {
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend((payload.len() as i32).to_be_bytes());
        bytes.extend([0; 4]);
        bytes.extend(payload);
    }
    let archive = FrmArchive::parse(&bytes).unwrap();
    assert_eq!(archive.frame(0, 0).unwrap().pixels(), &[5, 6]);
    let empty = archive.frame(0, 1).unwrap();
    assert_eq!(
        (empty.width, empty.height, empty.file_offset),
        (0, 0, 0x3e + 15)
    );
    assert!(empty.pixels().is_empty());
}

#[test]
fn spec001_lst_positions_consumer_ids_and_crlf() {
    for crlf in [false, true] {
        let convert = |text: &str| {
            if crlf {
                text.replace('\n', "\r\n").into_bytes()
            } else {
                text.as_bytes().to_vec()
            }
        };
        let art = LstTable::parse(
            &convert(include_str!("../../../tests/fixtures/formats/art.lst")),
            LstKind::Art,
        )
        .unwrap();
        let expected: [&[u8]; 6] = [b"alpha.frm", b"", b"", b"#note", b"", b"gamma.frm"];
        assert_eq!(art.entries().len(), 6);
        for (id, name) in expected.into_iter().enumerate() {
            assert_eq!(
                art.art_id(id as u32 | 0x1000).unwrap().name.as_deref(),
                Some(name)
            );
        }
        assert_eq!(art.entries()[5].critter_alias(99), 7);
        assert_eq!(art.entries()[0].critter_alias(99), 99);
        let proto = LstTable::parse(
            &convert(include_str!("../../../tests/fixtures/formats/proto.lst")),
            LstKind::Prototype,
        )
        .unwrap();
        assert!(proto.prototype_id(0).is_none());
        assert!(proto.prototype_id(4).is_none());
        for (id, name) in [b"one.pro".as_slice(), b"", b"two.pro"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                proto
                    .prototype_id((id + 1) as u32 | 0x01000000)
                    .unwrap()
                    .name
                    .as_deref(),
                Some(name)
            );
        }
        let scripts = LstTable::parse(
            &convert(include_str!("../../../tests/fixtures/formats/scripts.lst")),
            LstKind::Script,
        )
        .unwrap();
        assert_eq!(
            scripts.script_id(0).unwrap().name.as_deref(),
            Some(b"first.int".as_slice())
        );
        assert_eq!(scripts.script_id(0).unwrap().local_vars, Some(3));
        assert_eq!(
            scripts.map_script_reference(2).unwrap().name.as_deref(),
            Some(b"second.int".as_slice())
        );
        assert_eq!(scripts.map_script_reference(1), scripts.script_id(0));
        assert!(scripts.map_script_reference(0).is_none());
        // Entry counts come from successful reads, not LF + 1 / LF + 2.
        assert_eq!(scripts.bookkeeping_len(), scripts.entries().len());
        assert_eq!(proto.bookkeeping_len(), 3);
    }
}

#[test]
fn lst_bounded_reads_metadata_and_unusual_whitespace() {
    for (kind, limit) in [
        (LstKind::Art, 199),
        (LstKind::Script, 259),
        (LstKind::Prototype, 255),
    ] {
        let mut bytes = vec![b'x'; limit];
        bytes.extend_from_slice(b"\nend");
        let table = LstTable::parse(&bytes, kind).unwrap();
        assert_eq!(table.entries().len(), 3);
        assert_eq!(table.entries()[1].raw, b"\n");
    }
    let art = LstTable::parse(b"abcdefghijklmnop.frm\nhead,2,3,4\n//note\n", LstKind::Art).unwrap();
    assert_eq!(
        art.entries()[0].name.as_deref(),
        Some(b"abcdefghijkl".as_slice())
    );
    assert_eq!(art.entries()[1].head_counts, Some([2, 2, 2]));
    assert_eq!(art.entries()[1].head_columns, Some([2, 3, 4]));
    let heads = LstTable::parse(b"a,5\nb,2,x\nc\n", LstKind::Art).unwrap();
    assert_eq!(heads.entries()[0].head_columns, Some([5, 5, 5]));
    assert_eq!(heads.entries()[1].head_columns, Some([2, 0, 2]));
    assert_eq!(heads.entries()[2].head_columns, None);
    assert_eq!(art.entries()[2].name.as_deref(), Some(b"//note".as_slice()));
    let proto = LstTable::parse(b"a\tb;#x,\r\n", LstKind::Prototype).unwrap();
    assert_eq!(
        proto.entries()[0].name.as_deref(),
        Some(b"a\tb;#x,".as_slice())
    );
    let script = LstTable::parse(
        b" no-dot\n # hi.ssl # local_vars= -12more\n",
        LstKind::Script,
    )
    .unwrap();
    assert!(script.entries()[0].name.is_none());
    assert_eq!(
        script.entries()[1].name.as_deref(),
        Some(b" # hi.int".as_slice())
    );
    assert_eq!(script.entries()[1].local_vars, Some(-12));
    let empty = LstTable::parse(b"", LstKind::Script).unwrap();
    assert!(empty.entries().is_empty());
    assert_eq!(empty.bookkeeping_len(), 0);
}

#[test]
fn spec002_fon_bits_row_padding_metrics_and_mutations() {
    let bytes = fixtures::fon();
    assert_eq!(bytes.len(), 562);
    let font = Font::parse_fon(&bytes).unwrap();
    assert_eq!(
        (font.glyph_count(), font.height(), font.spacing()),
        (67, 2, 1)
    );
    assert_eq!(font.bitmap_bytes(), &[0x80, 0xff, 0x7f, 0, 0xa0, 0x40]);
    for (code, rows) in [(b'A', ["#.......#", ".#######."]), (b'B', ["#.#", ".#."])] {
        for (y, row) in rows.into_iter().enumerate() {
            for (x, painted) in row.bytes().enumerate() {
                assert_eq!(
                    font.coverage(code, x as u32, y as u32),
                    Some(if painted == b'#' { 7 } else { 0 })
                );
            }
        }
    }
    assert_eq!(
        (
            font.text_width(b"AB", false),
            font.buffer_size(b"AB", false),
            font.text_width(b"AB", true)
        ),
        (14, 28, 20)
    );
    assert_eq!(font.layout(b"AB", 13, false).glyphs.len(), 1);
    assert!(font.layout(b"AB", 9, false).glyphs.is_empty());
    assert_eq!(font.text_width(b"ABC", false), 14);
    assert_eq!(font.text_width(b"ABC", true), 30);
    assert_eq!(font.layout(b"ABC", 100, true).glyphs.len(), 2);
    assert_eq!(font.text_width(b"A\0B", false), 10);
    assert!(Font::parse_fon(&bytes[..561]).is_err());
    let mut extended = bytes.clone();
    extended.extend_from_slice(b"ignored");
    assert_eq!(
        Font::parse_fon(&extended).unwrap().bitmap_bytes(),
        font.bitmap_bytes()
    );
    let mut short_extent = bytes;
    short_extent[0x228..0x22c].fill(0);
    assert!(Font::parse_fon(&short_extent).is_err());
}

#[test]
fn spec002_aaf_pixels_alignment_spacing_and_palette_blending() {
    let font = Font::parse_aaf(&fixtures::aaf()).unwrap();
    assert_eq!(font.glyph_count(), 256);
    assert_eq!(
        (
            font.height(),
            font.line_height(),
            font.character_width(b' ')
        ),
        (3, 4, Some(2))
    );
    assert_eq!(
        (
            font.text_width(b"A B", false),
            font.buffer_size(b"A B", false),
            font.max_cell_width(),
            font.text_width(b"A B", true)
        ),
        (10, 40, 4, 12)
    );
    let layout = font.layout(b"A B", 10, false);
    assert_eq!(
        layout.glyphs.iter().map(|g| (g.x, g.y)).collect::<Vec<_>>(),
        [(0, 1), (4, 3), (7, 2)]
    );
    assert!(font.layout(b"A", 3, false).glyphs.is_empty());
    assert_eq!(font.layout(b"A", 4, false).glyphs.len(), 1);
    let palette = Palette::parse(&fixtures::grayscale_palette()).unwrap();
    let coverage: Vec<_> = (0..2)
        .flat_map(|y| (0..3).map(move |x| (x, y)))
        .map(|(x, y)| font.coverage(b'A', x, y).unwrap())
        .collect();
    assert_eq!(coverage, [0, 1, 7, 8, 3, 0]);
    assert_eq!(
        coverage
            .iter()
            .map(|level| palette.blend_coverage(*level, 28, 0).unwrap())
            .collect::<Vec<_>>(),
        [0, 4, 28, 27, 12, 0]
    );
    assert_eq!(font.coverage(b'B', 0, 0), Some(7));
    assert_eq!(font.coverage(b'B', 1, 0), Some(0));
    assert_eq!(palette.blend_coverage(14, 28, 0), None);
    let mut swapped_signature = fixtures::aaf();
    swapped_signature[..4].copy_from_slice(b"FFAA");
    assert!(Font::parse_aaf(&swapped_signature).is_err());
    for (offset, value) in [(0x80c, 0x0e), (0x80c, 0x10)] {
        let mut bytes = fixtures::aaf();
        bytes[offset] = value;
        assert!(Font::parse_aaf(&bytes).is_err());
    }
    assert!(Font::parse_aaf(&fixtures::aaf()[..0x800]).is_err());
    assert!(Font::parse_aaf(&fixtures::aaf()[..0x813]).is_err());
}

#[test]
fn spec002_msg_multiline_duplicates_and_false_comments() {
    let text = include_str!("../../../tests/fixtures/formats/messages.msg");
    for (bytes, strict) in [
        text.as_bytes().to_vec(),
        text.replace('\n', "\r\n").into_bytes(),
    ]
    .into_iter()
    .flat_map(|bytes| [(bytes.clone(), false), (bytes, true)])
    {
        // The authoring (strict) reader accepts the same valid fixture records.
        let catalog = if strict {
            MsgCatalog::parse_strict(&bytes).unwrap()
        } else {
            MsgCatalog::parse(&bytes).unwrap()
        };
        assert_eq!(catalog.entries().len(), 5);
        for (id, sound, text) in [
            (2, "", " keep  spaces "),
            (10, "beep", "Northeast"),
            (11, "", "Last"),
            (12, "", "Still parsed"),
            (13, "", "a{b"),
        ] {
            let record = catalog.get(id).unwrap();
            assert_eq!(record.sound, sound.as_bytes());
            assert_eq!(record.text, text.as_bytes());
        }
    }
}

#[test]
fn spec002_msg_exact_bytes_and_acceptance_boundaries() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/formats/message-bytes.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let bytes = fixtures::hex(case["hex"].as_str().unwrap());
        for catalog in [
            MsgCatalog::parse(&bytes).unwrap(),
            MsgCatalog::parse_strict(&bytes).unwrap(),
        ] {
            let message = catalog.get(case["id"].as_i64().unwrap() as i32).unwrap();
            assert!(message.sound.is_empty());
            assert_eq!(message.text, fixtures::hex(case["text"].as_str().unwrap()));
        }
    }
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/formats/message-boundaries.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let mut catalog = MsgCatalog::default();
        let result = catalog.load_strict(
            case["input"].as_str().unwrap().as_bytes(),
            TextMode::Archive,
        );
        assert_eq!(
            result.is_err(),
            case["error"].as_bool().unwrap_or(false),
            "{case}"
        );
        let expected: Vec<i32> = case["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_i64().unwrap() as i32)
            .collect();
        assert_eq!(
            catalog.entries().keys().copied().collect::<Vec<_>>(),
            expected,
            "{case}"
        );
    }
    for line in include_str!("../../../tests/fixtures/formats/message-lengths.txt").lines() {
        let (length, outcome) = line.split_once(' ').unwrap();
        let mut bytes = b"{30}{}{".to_vec();
        bytes.extend(std::iter::repeat_n(b'A', length.parse().unwrap()));
        bytes.push(b'}');
        assert_eq!(MsgCatalog::parse_strict(&bytes).is_ok(), outcome == "ok");
    }
    let bytes = fixtures::sparse(include_str!(
        "../../../tests/fixtures/formats/message-eof.hex"
    ));
    assert!(
        MsgCatalog::parse_with_mode(&bytes, TextMode::WindowsLoose)
            .unwrap()
            .entries()
            .is_empty()
    );
    assert_eq!(
        MsgCatalog::parse(&bytes).unwrap().get(1).unwrap().text,
        b"x"
    );
    for id in ["2147483648", "-2147483649"] {
        assert!(MsgCatalog::parse_strict(format!("{{{id}}}{{}}{{x}}").as_bytes()).is_err());
    }
    let mut catalog = MsgCatalog::parse(b"{1}{sound}{old}").unwrap();
    assert!(
        catalog
            .load_strict(b"{2}{}{other}{1}{}{} }", TextMode::Archive)
            .is_err()
    );
    assert_eq!(catalog.get(1).unwrap().text, b"");
    assert_eq!(catalog.get(1).unwrap().sound, b"");
    assert_eq!(catalog.get(2).unwrap().text, b"other");
}

#[test]
fn compatible_messages_trim_ids_skip_corrupt_records_and_recover_without_shifting_fields() {
    let fixture = include_str!("../../../tests/fixtures/formats/messages-compatible.msg");
    for bytes in [
        fixture.as_bytes().to_vec(),
        fixture.replace('\n', "\r\n").into_bytes(),
    ] {
        let catalog = MsgCatalog::parse(&bytes).unwrap();
        assert_eq!(catalog.warning_count(), 9);
        assert_eq!(catalog.warnings().len(), 9);
        assert!(
            catalog
                .warnings()
                .iter()
                .all(|warning| warning.offset < bytes.len())
        );
        assert_eq!(
            catalog.entries().keys().copied().collect::<Vec<_>>(),
            [
                -7, 0, 8, 100, 101, 102, 103, 105, 106, 107, 108, 109, 110, 112, 114, 115
            ]
        );
        for (id, text) in [
            (0, "Zero stays"),
            (-7, "Negative"),
            (8, "Positive"),
            (100, "Replacement"),
            (101, "Trailing"),
            (102, "After empty"),
            (103, "After annotation"),
            (105, "After broken ID"),
            (106, ""),
            (107, "After extra field"),
            (108, "voice"),
            (109, "After extra field"),
            (110, "After unmatched"),
            (112, "After missing text"),
            (114, "After unclosed"),
            (115, "After overflow"),
        ] {
            assert_eq!(catalog.get(id).unwrap().text, text.as_bytes(), "ID {id}");
        }
        assert_eq!(catalog.get(-7).unwrap().sound, b"tone");
        assert!(MsgCatalog::parse_strict(&bytes).is_err());
        validate_asset(&VfsPath::new("text/dialog/synthetic.msg").unwrap(), &bytes)
            .unwrap()
            .unwrap();
    }
    let catalog =
        MsgCatalog::parse(b"{ \t100 \t}{}{Padded}\n{1}\n{42}\n{Numeric sound}\n{2}{}{End}")
            .unwrap();
    assert_eq!(catalog.get(100).unwrap().text, b"Padded");
    assert_eq!(catalog.get(1).unwrap().sound, b"42");
    assert_eq!(catalog.get(2).unwrap().text, b"End");
    assert_eq!(catalog.warning_count(), 0);
}

#[test]
fn compatible_message_recovery_keeps_resource_and_diagnostic_buffers_bounded() {
    let mut oversized = b"{1}{}{".to_vec();
    oversized.extend(std::iter::repeat_n(b'A', 1024));
    oversized.extend_from_slice(b"}\n{2}{}{kept}");
    let catalog = MsgCatalog::parse(&oversized).unwrap();
    assert!(catalog.get(1).is_none());
    assert_eq!(catalog.get(2).unwrap().text, b"kept");
    assert_eq!(catalog.warning_count(), 1);
    assert!(MsgCatalog::parse_strict(&oversized).is_err());

    let malformed = b"{}{}{ignored}\n{1}{}{kept}\n".repeat(256);
    let catalog = MsgCatalog::parse(&malformed).unwrap();
    assert_eq!(catalog.warning_count(), 256);
    assert_eq!(catalog.warnings().len(), 128);
    assert_eq!(catalog.entries().len(), 1);
    assert_eq!(catalog.get(1).unwrap().text, b"kept");
}

#[test]
fn every_truncated_binary_fixture_is_rejected() {
    type Parser = fn(&[u8]) -> Result<(), ParseError>;
    let cases: [(Vec<u8>, Parser); 5] = [
        (
            fixtures::sparse(include_str!(
                "../../../tests/fixtures/formats/palette-base.hex"
            )),
            |bytes| Palette::parse(bytes).map(|_| ()),
        ),
        (fixtures::shared_frm(), |bytes| {
            FrmArchive::parse(bytes).map(|_| ())
        }),
        (fixtures::animated_frm(), |bytes| {
            FrmArchive::parse(bytes).map(|_| ())
        }),
        (fixtures::fon(), |bytes| Font::parse_fon(bytes).map(|_| ())),
        (fixtures::aaf(), |bytes| Font::parse_aaf(bytes).map(|_| ())),
    ];
    for (bytes, parse) in cases {
        for end in 0..bytes.len() {
            assert!(
                parse(&bytes[..end]).is_err(),
                "accepted truncated prefix {end}"
            );
        }
        parse(&bytes).unwrap();
    }
}

#[test]
fn bounded_mutation_and_random_byte_fuzz_smoke() {
    fn exercise(bytes: &[u8]) {
        let _ = Palette::parse(bytes);
        let _ = FrmArchive::parse(bytes);
        let _ = Font::parse_fon(bytes);
        let _ = Font::parse_aaf(bytes);
        let _ = MsgCatalog::parse(bytes);
        let _ = MsgCatalog::parse_strict(bytes);
        for kind in [
            LstKind::Raw,
            LstKind::Art,
            LstKind::Script,
            LstKind::Prototype,
        ] {
            let _ = LstTable::parse(bytes, kind);
        }
    }
    let mut state = 0xc0ffee_u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    for length in 0..512 {
        exercise(&(0..length).map(|_| next() as u8).collect::<Vec<_>>());
    }
    for original in [
        fixtures::shared_frm(),
        fixtures::animated_frm(),
        fixtures::fon(),
        fixtures::aaf(),
    ] {
        for _ in 0..256 {
            let mut bytes = original.clone();
            let offset = next() as usize % bytes.len();
            bytes[offset] = next() as u8;
            exercise(&bytes);
        }
    }
    let oversized = vec![0; MAX_ASSET_BYTES + 1];
    assert!(Palette::parse(&oversized).is_err());
    assert!(Font::parse_fon(&oversized).is_err());
    assert!(Font::parse_aaf(&oversized).is_err());
    assert!(FrmArchive::parse(&oversized).is_err());
    assert!(MsgCatalog::parse(&oversized).is_err());
    assert!(LstTable::parse(&oversized, LstKind::Raw).is_err());
}

#[test]
fn validation_dispatch_covers_every_supported_extension() {
    let temp = tempfile::tempdir().unwrap();
    fixtures::write_corpus(temp.path());
    let vfs = LocalDirProvider::mount(temp.path()).unwrap();
    assert_eq!(vfs.files().count(), 16);
    for path in vfs.files() {
        validate_asset(path, &vfs.read(path.as_str()).unwrap())
            .unwrap()
            .unwrap();
    }
    assert!(validate_asset(&VfsPath::new("unknown.dat").unwrap(), b"").is_none());
    assert_eq!(
        LstKind::for_path(&VfsPath::new("data/custom.lst").unwrap()),
        LstKind::Raw
    );
}

#[test]
fn spec002_font_startup_search_defaults_and_numbered_selection() {
    use crate::{FontSet, TextFlags, font::FontKind};
    let dir = tempfile::tempdir().unwrap();
    // FON search covers suffixes 0-9 (font7 is beyond the shipped 0-5 names);
    // AAF search covers 0-15, but only 100-110 are registered selections.
    std::fs::write(dir.path().join("font7.fon"), fixtures::fon()).unwrap();
    std::fs::write(dir.path().join("font2.fon"), fixtures::fon()).unwrap();
    std::fs::write(dir.path().join("font1.fon"), b"invalid").unwrap();
    std::fs::write(dir.path().join("font12.fon"), fixtures::fon()).unwrap();
    std::fs::write(dir.path().join("font3.aaf"), fixtures::aaf()).unwrap();
    std::fs::write(dir.path().join("font13.aaf"), fixtures::aaf()).unwrap();
    let vfs = LocalDirProvider::mount(dir.path()).unwrap();
    let mut fonts = FontSet::load(&vfs);
    // Lowest successfully loaded suffix: the invalid font1.fon is skipped.
    assert_eq!(fonts.default_font(FontKind::Bitmap).map(|f| f.0), Some(2));
    assert_eq!(
        fonts.default_font(FontKind::Antialiased).map(|f| f.0),
        Some(3)
    );
    assert!(fonts.get(7).is_some() && fonts.get(12).is_none() && fonts.get(1).is_none());
    assert!(fonts.get(103).is_some() && fonts.get(113).is_none() && fonts.get(3).is_none());
    assert_eq!(fonts.current_number(), None);
    assert!(fonts.select(103));
    assert_eq!(fonts.current().unwrap().kind(), FontKind::Antialiased);
    // A missing slot does not replace the selection with a nearby font.
    for missing in [100, 104, 1, 5, 12, 111, -1] {
        assert!(!fonts.select(missing));
        assert_eq!(fonts.current_number(), Some(103));
    }
    assert!(fonts.select(7));
    assert_eq!(fonts.current().unwrap().kind(), FontKind::Bitmap);
    // No loaded fonts: each subsystem reports initialization failure.
    let empty = tempfile::tempdir().unwrap();
    let fonts = FontSet::load(&LocalDirProvider::mount(empty.path()).unwrap());
    assert!(fonts.default_font(FontKind::Bitmap).is_none());
    assert!(fonts.default_font(FontKind::Antialiased).is_none());

    let flags = TextFlags::from_word(0x0007_00D7);
    assert_eq!(
        flags,
        TextFlags {
            foreground: 0xD7,
            shadow: true,
            underline: true,
            monospaced: true
        }
    );
    assert_eq!(
        TextFlags::from_word(0x0002_0105),
        TextFlags {
            foreground: 5,
            shadow: false,
            underline: true,
            monospaced: false
        }
    );
}
