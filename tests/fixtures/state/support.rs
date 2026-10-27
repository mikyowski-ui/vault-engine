pub use crate::formats as assets;
pub use crate::world;

pub fn gcd() -> Vec<u8> {
    assets::sparse(include_str!("player.gcd.hex"))
}

/// [`gcd`] with another name, tags and traits. With `extra_word`, it is
/// SPEC-018 §6.3's 432-byte Fallout 2 sheet (Revision 4): that word goes in
/// at `0x170`, and the name, tags, traits and points move down 4 bytes.
pub fn sheet_gcd(
    name: &[u8],
    tags: [i32; 4],
    traits: [i32; 2],
    extra_word: Option<i32>,
) -> Vec<u8> {
    let mut bytes = gcd();
    bytes[0x170..0x190].fill(0);
    bytes[0x170..0x170 + name.len()].copy_from_slice(name);
    for (index, word) in tags.iter().chain(&traits).enumerate() {
        world::word(&mut bytes, 0x190 + 4 * index, *word);
    }
    if let Some(extra_word) = extra_word {
        bytes.splice(0x170..0x170, extra_word.to_be_bytes());
    }
    bytes
}

/// A synthetic premade, in SPEC-014 §3.7's entry order: its name, three
/// distinct tags, its traits and its portrait's one color.
pub struct SyntheticPremade {
    pub name: &'static [u8],
    pub tags: [i32; 4],
    pub traits: [i32; 2],
    pub color: u8,
}
pub const PREMADES: [SyntheticPremade; 3] = [
    SyntheticPremade {
        name: b"Fixture Fighter",
        tags: [1, 11, 17, -1],
        traits: [3, 9],
        color: 40,
    },
    SyntheticPremade {
        name: b"Fixture Sneak",
        tags: [2, 12, 16, -1],
        traits: [5, -1],
        color: 41,
    },
    SyntheticPremade {
        name: b"Fixture Talker",
        tags: [7, 13, 0, -1],
        traits: [-1, -1],
        color: 42,
    },
];
/// The selection background's one color.
pub const PICKCHAR_COLOR: u8 = 9;
/// The premade selection screen's files (SPEC-014 §3.7, SPEC-019 §3.3):
/// `pickchar.frm`, and for each of [`PREMADES`] its sheet, a one-line bio
/// and a one-color portrait. Fallout 1's portraits are 212 by 187 in
/// 428-byte sheets. Fallout 2's are 592 by 260, the size of its monitor
/// window, in 432-byte sheets, except the third, which is 640 by 300 so
/// that a test sees it clipped.
pub fn premade_files(fallout2: bool) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![(
        "art/intrface/pickchar.frm".to_owned(),
        world::frm(640, 480, PICKCHAR_COLOR),
    )];
    let entries = ["combat", "stealth", "diplomat"];
    for (index, (entry, premade)) in entries.iter().zip(&PREMADES).enumerate() {
        let (size, extra_word) = match (fallout2, index) {
            (false, _) => ((212, 187), None),
            (true, 2) => ((640, 300), Some(2)),
            (true, _) => ((592, 260), Some(2)),
        };
        files.push((
            format!("art/intrface/{entry}.frm"),
            world::frm(size.0, size.1, premade.color),
        ));
        files.push((
            format!("premade/{entry}.gcd"),
            sheet_gcd(premade.name, premade.tags, premade.traits, extra_word),
        ));
        files.push((
            format!("premade/{entry}.bio"),
            b"A synthetic bio.\n".to_vec(),
        ));
    }
    files
}
pub fn instruction(op: u16) -> Vec<u8> {
    op.to_be_bytes().to_vec()
}
pub fn push(value: i32) -> Vec<u8> {
    let mut bytes = instruction(0xc001);
    bytes.extend(value.to_be_bytes());
    bytes
}
pub fn literal(offset: u32) -> Vec<u8> {
    let mut bytes = instruction(0x9001);
    bytes.extend(offset.to_be_bytes());
    bytes
}
fn table(strings: &[&[u8]]) -> (Vec<u8>, Vec<u32>) {
    let mut payload = Vec::new();
    let mut offsets = Vec::new();
    for text in strings {
        offsets.push(payload.len() as u32 + 6);
        let size = (text.len() + 1).div_ceil(2) * 2;
        payload.extend((size as u16).to_be_bytes());
        payload.extend(*text);
        payload.resize(payload.len() + size - text.len(), 0);
    }
    let mut bytes = (payload.len() as u32).to_be_bytes().to_vec();
    bytes.extend(payload);
    bytes.extend([255; 4]);
    (bytes, offsets)
}
pub fn program(strings: &[&[u8]], procedures: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
    let bootstrap = assets::sparse(include_str!("../vm/program.int.hex"));
    let count = procedures.len() + 1;
    let mut bytes = bootstrap[..42].to_vec();
    bytes.extend((count as u32).to_be_bytes());
    bytes.resize(46 + 24 * count, 0);
    let mut names = vec![b"main".as_slice()];
    names.extend(procedures.iter().map(|(name, _)| *name));
    let (identifiers, name_offsets) = table(&names);
    bytes.extend(identifiers);
    bytes.extend(table(strings).0);
    let init = bytes.len();
    world::word(&mut bytes, 12, init as i32);
    let root = init + 18;
    for code in [
        instruction(0x802c),
        push(0),
        instruction(0x8003),
        push(root as i32),
        instruction(0x8004),
    ] {
        bytes.extend(code);
    }
    let mut addresses = vec![root as u32];
    bytes.extend(
        [
            instruction(0x802b),
            instruction(0x802a),
            instruction(0x8029),
            instruction(0x801c),
        ]
        .concat(),
    );
    for (_, body) in procedures {
        addresses.push(bytes.len() as u32);
        bytes.extend(body);
    }
    for index in 0..count {
        world::word(&mut bytes, 46 + 24 * index, name_offsets[index] as i32);
        world::word(&mut bytes, 46 + 24 * index + 16, addresses[index] as i32);
    }
    bytes
}
pub fn returning(body: Vec<u8>) -> Vec<u8> {
    [
        instruction(0x802b),
        body,
        push(0),
        instruction(0x800d),
        instruction(0x802a),
        instruction(0x800c),
        instruction(0x8029),
        instruction(0x801c),
    ]
    .concat()
}
pub fn context_program(strings: &[&[u8]]) -> Vec<u8> {
    program(strings, &[(b"start", returning(Vec::new()))])
}
pub fn dialogue_program(head: i32) -> Vec<u8> {
    let talk = [
        push(1),
        instruction(0x80bc),
        push(0),
        push(head),
        push(0),
        instruction(0x80de),
        instruction(0x811c),
        push(1),
        literal(6),
        instruction(0x811e),
        push(1),
        literal(14),
        push(3),
        push(50),
        instruction(0x811f),
        instruction(0x811d),
        instruction(0x80df),
    ]
    .concat();
    let next = [
        push(1),
        literal(6),
        instruction(0x811e),
        push(1),
        literal(14),
        push(0),
        push(50),
        instruction(0x811f),
    ]
    .concat();
    program(
        &[b"Hello", b"Continue"],
        &[
            (b"start", returning(Vec::new())),
            (b"talk_p_proc", returning(talk)),
            (b"next_reply", returning(next)),
        ],
    )
}
pub fn npc_prototype() -> Vec<u8> {
    let mut bytes = world::prototype(1, 0);
    // Extended flag 0x4000: the critter can be talked to.
    world::word(&mut bytes, 0x18, 0x4000);
    for stat in 0..7 {
        world::word(&mut bytes, 0x30 + stat * 4, 5);
    }
    world::word(&mut bytes, 0x30 + 7 * 4, 30);
    world::word(&mut bytes, 0x30 + 8 * 4, 7);
    world::word(&mut bytes, 0x30 + 33 * 4, 25);
    bytes
}
pub fn interactive_map() -> Vec<u8> {
    let mut bytes = world::preview_map();
    bytes.truncate(248 + 120000);
    bytes.extend(world::words(&[0, 0, 0, 0, 1]));
    let mut slot = world::script_slot(4, 1);
    world::word(&mut slot, 20, 2);
    world::word(&mut slot, 28, 4);
    bytes.extend(slot);
    for _ in 1..16 {
        bytes.extend(world::script_slot(0, 0));
    }
    bytes.extend(world::words(&[1, 0]));
    bytes.extend(world::words(&[5, 5]));
    for (category, hex, id) in [
        (0, 20100, 1),
        (1, 20099, 2),
        (2, 20301, 3),
        (3, 20101, 4),
        (0, 20300, 5),
    ] {
        let mut object = world::object(category, hex, id, if category == 2 { &[0] } else { &[] });
        if category == 1 {
            world::word(&mut object, 0x40, 0x04000001);
            world::word(&mut object, 0x44, 0);
            world::word(&mut object, 0x74, 30);
        } else {
            world::word(&mut object, 0x24, 16);
        }
        bytes.extend(object);
    }
    bytes.extend(world::words(&[0, 0]));
    bytes
}
pub fn animated_frame(width: u16, height: u16, colors: &[u8]) -> Vec<u8> {
    let mut bytes = world::frm(width, height, colors[0]);
    bytes.truncate(62);
    bytes[8..10].copy_from_slice(&(colors.len() as u16).to_be_bytes());
    world::word(
        &mut bytes,
        58,
        colors.len() as i32 * (12 + i32::from(width) * i32::from(height)),
    );
    for color in colors {
        let frame = world::frm(width, height, *color);
        bytes.extend_from_slice(&frame[62..]);
    }
    bytes
}
pub fn preview_palette() -> Vec<u8> {
    let mut bytes = assets::palette();
    for index in 0..256 {
        bytes[index * 3] = ((index >> 5) & 7) as u8 * 9;
        bytes[index * 3 + 1] = ((index >> 2) & 7) as u8 * 9;
        bytes[index * 3 + 2] = (index & 3) as u8 * 21;
    }
    for r in 0..32 {
        for g in 0..32 {
            for b in 0..32 {
                bytes[768 + r * 1024 + g * 32 + b] =
                    (((r * 7 / 31) << 5) | ((g * 7 / 31) << 2) | (b * 3 / 31)) as u8;
            }
        }
    }
    bytes
}
pub fn preview_font() -> Vec<u8> {
    // Independently drawn 5x7 ASCII glyphs. Lowercase uses the same synthetic caps.
    let patterns = [
        ('A', [14, 17, 17, 31, 17, 17, 17]),
        ('B', [30, 17, 17, 30, 17, 17, 30]),
        ('C', [14, 17, 16, 16, 16, 17, 14]),
        ('D', [30, 17, 17, 17, 17, 17, 30]),
        ('E', [31, 16, 16, 30, 16, 16, 31]),
        ('F', [31, 16, 16, 30, 16, 16, 16]),
        ('G', [14, 17, 16, 23, 17, 17, 15]),
        ('H', [17, 17, 17, 31, 17, 17, 17]),
        ('I', [14, 4, 4, 4, 4, 4, 14]),
        ('J', [7, 2, 2, 2, 2, 18, 12]),
        ('K', [17, 18, 20, 24, 20, 18, 17]),
        ('L', [16, 16, 16, 16, 16, 16, 31]),
        ('M', [17, 27, 21, 21, 17, 17, 17]),
        ('N', [17, 25, 21, 19, 17, 17, 17]),
        ('O', [14, 17, 17, 17, 17, 17, 14]),
        ('P', [30, 17, 17, 30, 16, 16, 16]),
        ('Q', [14, 17, 17, 17, 21, 18, 13]),
        ('R', [30, 17, 17, 30, 20, 18, 17]),
        ('S', [15, 16, 16, 14, 1, 1, 30]),
        ('T', [31, 4, 4, 4, 4, 4, 4]),
        ('U', [17, 17, 17, 17, 17, 17, 14]),
        ('V', [17, 17, 17, 17, 17, 10, 4]),
        ('W', [17, 17, 17, 21, 21, 21, 10]),
        ('X', [17, 17, 10, 4, 10, 17, 17]),
        ('Y', [17, 17, 10, 4, 4, 4, 4]),
        ('Z', [31, 1, 2, 4, 8, 16, 31]),
        ('0', [14, 17, 19, 21, 25, 17, 14]),
        ('1', [4, 12, 4, 4, 4, 4, 14]),
        ('2', [14, 17, 1, 2, 4, 8, 31]),
        ('3', [30, 1, 1, 14, 1, 1, 30]),
        ('4', [2, 6, 10, 18, 31, 2, 2]),
        ('5', [31, 16, 16, 30, 1, 1, 30]),
        ('6', [14, 16, 16, 30, 17, 17, 14]),
        ('7', [31, 1, 2, 4, 8, 8, 8]),
        ('8', [14, 17, 17, 14, 17, 17, 14]),
        ('9', [14, 17, 17, 15, 1, 1, 14]),
        ('.', [0, 0, 0, 0, 0, 12, 12]),
        (':', [0, 12, 12, 0, 12, 12, 0]),
        ('-', [0, 0, 0, 31, 0, 0, 0]),
        ('?', [14, 17, 1, 2, 4, 0, 4]),
        (' ', [0; 7]),
    ];
    let mut bytes = Vec::new();
    for word in [256_i32, 7, 1, 0, 0] {
        bytes.extend(word.to_le_bytes());
    }
    for index in 0..256 {
        bytes.extend((if index == 32 { 3_i32 } else { 5 }).to_le_bytes());
        bytes.extend((index * 7_i32).to_le_bytes());
    }
    for index in 0..256 {
        let character = (index as u8).to_ascii_uppercase() as char;
        let rows = if index == 0x95 {
            [0, 0, 14, 14, 14, 0, 0]
        } else {
            patterns
                .iter()
                .find(|(code, _)| *code == character)
                .map_or([0; 7], |(_, rows)| *rows)
        };
        bytes.extend(rows.map(|row| row << 3));
    }
    bytes
}
pub fn preview_aaf() -> Vec<u8> {
    let font = preview_font();
    let bits = &font[20 + 256 * 8..];
    let mut bytes = b"AAFF".to_vec();
    for word in [7_i16, 1, 3, 1] {
        bytes.extend(word.to_be_bytes());
    }
    let mut pixels = Vec::new();
    for code in 0..256 {
        let width = if code == 32 { 3_i16 } else { 5 };
        let height = if code == 32 { 0_i16 } else { 7 };
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend((pixels.len() as i32).to_be_bytes());
        for y in 0..height as usize {
            for x in 0..width {
                pixels.push(if bits[code * 7 + y] & (0x80 >> x) != 0 {
                    7_u8
                } else {
                    0
                });
            }
        }
    }
    bytes.extend(pixels);
    bytes
}
pub fn write_corpus(root: &std::path::Path) {
    world::write_corpus(root);
    std::fs::write(root.join("color.pal"), preview_palette()).unwrap();
    std::fs::write(root.join("font0.fon"), preview_font()).unwrap();
    std::fs::write(root.join("font0.aaf"), preview_aaf()).unwrap();
    std::fs::create_dir_all(root.join("data")).unwrap();
    std::fs::create_dir_all(root.join("premade")).unwrap();
    std::fs::write(
        root.join("data/vault13.gam"),
        include_bytes!("campaign.gam"),
    )
    .unwrap();
    std::fs::write(root.join("maps/synth.gam"), include_bytes!("map.gam")).unwrap();
    std::fs::write(root.join("premade/synthetic.gcd"), gcd()).unwrap();
    std::fs::write(root.join("maps/synthetic.map"), interactive_map()).unwrap();
    std::fs::write(
        root.join("scripts/scripts.lst"),
        b"synthetic.int # local_vars=4\n",
    )
    .unwrap();
    std::fs::write(root.join("scripts/synthetic.int"), dialogue_program(0)).unwrap();
    // Start a no-head dialogue in a second image's initial stream for GPU smoke
    // coverage, without injecting input into the user's desktop.
    let mut auto = dialogue_program(-1);
    let count = i32::from_be_bytes(auto[42..46].try_into().unwrap()) as usize;
    let identifiers = 46 + 24 * count;
    let literals = identifiers
        + 8
        + i32::from_be_bytes(auto[identifiers..identifiers + 4].try_into().unwrap()) as usize;
    let init = literals
        + 8
        + i32::from_be_bytes(auto[literals..literals + 4].try_into().unwrap()) as usize;
    let talk = i32::from_be_bytes(auto[46 + 24 * 2 + 16..46 + 24 * 2 + 20].try_into().unwrap());
    world::word(&mut auto, init + 12, talk);
    std::fs::write(root.join("scripts/smoketalk.int"), auto).unwrap();
    std::fs::write(
        root.join("scripts/scripts.lst"),
        b"synthetic.int # local_vars=4\nsmoketalk.int # local_vars=4\n",
    )
    .unwrap();
    let mut map = interactive_map();
    world::word(&mut map, 248 + 120000 + 20 + 12, 1);
    std::fs::write(root.join("maps/dialogue.map"), map).unwrap();
    std::fs::write(root.join("proto/critters/synthetic.pro"), npc_prototype()).unwrap();
    std::fs::write(root.join("proto/items/synthetic.pro"), world::container()).unwrap();
    std::fs::write(
        root.join("proto/scenery/synthetic.pro"),
        world::prototype(2, 0),
    )
    .unwrap();
    std::fs::write(
        root.join("art/scenery/sample.frm"),
        animated_frame(12, 20, &[17, 25]),
    )
    .unwrap();
    for folder in ["heads", "backgrnd", "intrface"] {
        std::fs::create_dir_all(root.join(format!("art/{folder}"))).unwrap();
    }
    std::fs::write(root.join("art/heads/heads.lst"), b"synth,3,2,1\n").unwrap();
    std::fs::write(root.join("art/backgrnd/backgrnd.lst"), b"back.frm\n").unwrap();
    std::fs::write(root.join("art/backgrnd/back.frm"), world::frm(388, 200, 5)).unwrap();
    let mut interface = vec!["shell.frm"; 104];
    interface[99] = "panel.frm";
    std::fs::write(
        root.join("art/intrface/intrface.lst"),
        interface.join("\n") + "\n",
    )
    .unwrap();
    std::fs::write(root.join("art/intrface/shell.frm"), world::frm(640, 480, 0)).unwrap();
    std::fs::write(root.join("art/intrface/panel.frm"), world::frm(640, 145, 8)).unwrap();
    write_pipboy(root);
    for suffix in ["gf", "nf", "bf"] {
        for variant in 1..=3 {
            std::fs::write(
                root.join(format!("art/heads/synth{suffix}{variant}.frm")),
                animated_frame(96, 160, &[64 + variant, 96 + variant]),
            )
            .unwrap();
        }
    }
    for suffix in ["gv", "gn", "ng", "nb", "bn", "bv"] {
        std::fs::write(
            root.join(format!("art/heads/synth{suffix}.frm")),
            animated_frame(96, 160, &[128, 160]),
        )
        .unwrap();
    }
    for suffix in ["gp", "np", "bp"] {
        std::fs::write(
            root.join(format!("art/heads/synth{suffix}.frm")),
            animated_frame(96, 160, &[64, 65, 66, 67, 68, 69, 70, 71, 72]),
        )
        .unwrap();
    }
    for (file, name) in [
        ("item", "Box"),
        ("crit", "Guide"),
        ("scen", "Door"),
        ("wall", "Wall"),
    ] {
        std::fs::write(root.join(format!("text/english/game/pro_{file}.msg")),format!("{{100}}{{}}{{Synthetic {name}}}\n{{101}}{{}}{{A synthetic {name} for interaction tests.}}\n")).unwrap();
    }
}
/// SPEC-014 §3.41 the Pip-Boy's eleven art files at their corpus sizes,
/// each one color, and a `pipboy.msg` with the texts it shows.
pub fn write_pipboy(root: &std::path::Path) {
    for (name, width, height, color) in [
        ("pip.frm", 640, 480, 3),
        ("lilredup.frm", 15, 16, 30),
        ("lilreddn.frm", 15, 16, 31),
        ("numbers.frm", 360, 17, 32),
        ("pip2.frm", 148, 227, 33),
        ("months.frm", 29, 179, 34),
        ("notenums.frm", 120, 13, 35),
        ("alarmout.frm", 28, 24, 36),
        ("alarmin.frm", 28, 24, 37),
        ("pipx.frm", 204, 141, 38),
        ("bomb1.frm", 65, 65, 39),
    ] {
        std::fs::write(
            root.join(format!("art/intrface/{name}")),
            world::frm(width, height, color),
        )
        .unwrap();
    }
    std::fs::write(
        root.join("text/english/game/pipboy.msg"),
        "{107}{}{Merry Christmas!}\n{200}{}{MORE}\n{201}{}{BACK}\n{202}{}{STATUS}\n\
         {203}{}{No current tasks.}\n{205}{}{AUTOMAPS}\n{206}{}{VIDEO ARCHIVES}\n\
         {210}{}{Status}\n{211}{}{DATA}\n{212}{}{of}\n{214}{}{DONE}\n\
         {215}{}{You can not rest at this location!}\n{300}{}{ALARM CLOCK}\n\
         {301}{}{Hit Points}\n{302}{}{Rest for ten minutes}\n{314}{}{Rest until healed}\n\
         {400}{}{FEV Experiment Tape}\n{511}{}{Vault 13 Water}\n\
         {700}{}{Vault 13}\n{704}{}{Find the Water Chip.}\n",
    )
    .unwrap();
}

/// A corpus-shaped program (SPEC-004 §2.2): [`program`], with the initial
/// stream's jump going into the body of procedure 1 (normally `start`)
/// instead of the trampoline, so a first request runs `start` with
/// `script_action` returning the requested number (SPEC-006 §3.5).
pub fn corpus_program(strings: &[&[u8]], procedures: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
    let mut bytes = program(strings, procedures);
    let read = |bytes: &[u8], offset: usize| {
        i32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
    };
    let init = read(&bytes, 12) as usize;
    let start = read(&bytes, 46 + 24 + 16);
    // The initial stream is `0x802c`, push 0, `0x8003`, push target, `0x8004`.
    world::word(&mut bytes, init + 12, start);
    bytes
}

/// The `enhanced` camera demo, all synthetic. `maps/cam_demo.map` has one
/// elevation: scroll blockers ([`CAM_DEMO_BLOCKER_PID`], hidden) on the
/// ring of hex columns and rows [`CAM_DEMO_RING`] enclose the camera's
/// region, inside the map border and smaller than the 200-by-200 grid; the
/// enclosure has floor tiles; a walled, roofed building
/// ([`CAM_DEMO_BUILDING`], roof art index 2 over floor art index 0) has a
/// doorway at [`CAM_DEMO_DOOR`] and holds a critter and an item; and the
/// player enters outside it at [`CAM_DEMO_ENTRY`].
pub const CAM_DEMO_RING: [i32; 2] = [60, 140];
/// The building's walls in hex `[first column, last column, first row,
/// last row]`; the roof covers the squares under them.
pub const CAM_DEMO_BUILDING: [i32; 4] = [90, 109, 90, 107];
/// The doorway: the gap in the building's last wall row.
pub const CAM_DEMO_DOOR: i32 = 100 + 200 * 107;
/// The map's entry hex, in the enclosure and outside the building.
pub const CAM_DEMO_ENTRY: i32 = 100 + 200 * 125;
/// The interior hex the script pans to.
pub const CAM_DEMO_INTERIOR: i32 = 100 + 200 * 98;
/// The critter's and the item's hexes, inside the building.
pub const CAM_DEMO_CRITTER: i32 = 98 + 200 * 96;
pub const CAM_DEMO_ITEM: i32 = 103 + 200 * 100;
/// The scroll blocker PID (SPEC-014 §3.29).
pub const CAM_DEMO_BLOCKER_PID: u32 = 0x0500_000c;
/// The `scripts.lst` row of the demo's map script.
pub const CAM_DEMO_SCRIPT: &str = "camdemo.int # local_vars=0";
/// What the map script requests at map entry, in order, as the enhanced
/// word and its operands in the order written: pan to the interior over
/// 1500 ms, zoom to 200% over 1000 ms, shake with intensity 6 for 800 ms,
/// then reset over 1500 ms.
pub const CAM_DEMO_REQUESTS: [(u16, &[i32]); 4] = [
    (0x8210, &[CAM_DEMO_INTERIOR, 1500]),
    (0x8211, &[200, 1000]),
    (0x8212, &[6, 800]),
    (0x8213, &[1500]),
];

/// `scripts/camdemo.int`: `start` tests `script_action` for 15, the map
/// entry request, and then makes the four [`CAM_DEMO_REQUESTS`] at once
/// (their durations are presentation timing, which the view sequences),
/// as corpus map scripts do their entry work inside `start`. The startup
/// (1) and update (23) requests that reach `start` later skip them. The
/// words lie in the enhanced range, so only an enhanced parse accepts it.
pub fn cam_demo_program() -> Vec<u8> {
    let requests: Vec<u8> = CAM_DEMO_REQUESTS
        .iter()
        .flat_map(|(word, operands)| {
            let mut code: Vec<u8> = operands.iter().flat_map(|value| push(*value)).collect();
            code.extend(instruction(*word));
            code
        })
        .collect();
    let body = |skip: i32| {
        [
            push(skip),
            instruction(0x80c7),
            push(15),
            instruction(0x8033),
            instruction(0x802f),
            requests.clone(),
        ]
        .concat()
    };
    let build = |skip: i32| corpus_program(&[], &[(b"start", returning(body(skip)))]);
    // The skip target is the end of the requests: `start`'s address, its
    // `0x802b`, then the body. Its value does not change the layout.
    let first = build(0);
    let start = i32::from_be_bytes(first[46 + 24 + 16..46 + 24 + 20].try_into().unwrap());
    build(start + 2 + body(0).len() as i32)
}

/// `maps/cam_demo.map` with map script `scripts.lst` index `script`.
pub fn cam_demo_map(script: i32) -> Vec<u8> {
    let mut bytes = world::empty_map();
    bytes.truncate(236);
    bytes[4..20].fill(0);
    bytes[4..16].copy_from_slice(b"CAM_DEMO.MAP");
    for (offset, value) in [
        (0x14, CAM_DEMO_ENTRY),
        (0x18, 0),
        (0x1c, 0),
        // No local or map variables.
        (0x20, 0),
        (0x24, script + 1),
        // A fresh map with elevations 1 and 2 omitted.
        (0x28, 0x0c),
        (0x30, 0),
        (0x34, -1),
    ] {
        world::word(&mut bytes, offset, value);
    }
    let [ring_first, ring_last] = CAM_DEMO_RING.map(|hex| hex / 2);
    let [first_column, last_column, first_row, last_row] = CAM_DEMO_BUILDING;
    for position in 0..10000 {
        let (column, row) = (position % 100, position / 100);
        let inside = |first: i32, last: i32| {
            (first..=last).contains(&column) && (first..=last).contains(&row)
        };
        let roofed = (first_column / 2..=last_column / 2).contains(&column)
            && (first_row / 2..=last_row / 2).contains(&row);
        let word = if roofed {
            0x0002_0000_u32
        } else if inside(ring_first, ring_last) {
            0x0001_0000
        } else {
            0x0001_0001
        };
        bytes.extend(word.to_be_bytes());
    }
    bytes.extend(world::words(&[0; 5]));
    // (hex, PID category): the blockers (category 5), the walls but the
    // doorway (3), the critter (1) and the item (0).
    let [first, last] = CAM_DEMO_RING;
    let mut placed: Vec<(i32, u8)> = (first..=last)
        .flat_map(|index| [(index, first), (index, last), (first, index), (last, index)])
        .map(|(column, row)| (column + 200 * row, 5))
        .collect();
    placed.extend(
        (first_column..=last_column)
            .flat_map(|column| [(column, first_row), (column, last_row)])
            .chain(
                (first_row + 1..last_row).flat_map(|row| [(first_column, row), (last_column, row)]),
            )
            .map(|(column, row)| (column + 200 * row, 3))
            .filter(|(hex, _)| *hex != CAM_DEMO_DOOR),
    );
    placed.extend([(CAM_DEMO_CRITTER, 1), (CAM_DEMO_ITEM, 0)]);
    // In hex order, as the map writer stores objects, so the file reads
    // and writes back unchanged; the ring's corners appear once.
    placed.sort_unstable();
    placed.dedup();
    let mut objects = Vec::new();
    for (index, (hex, category)) in placed.into_iter().enumerate() {
        let mut object = world::object(category, hex, index as u32 + 1, &[]);
        match category {
            5 => {
                // Hidden, as scroll blockers are not drawn.
                world::word(&mut object, 0x24, 0x1);
                world::word(&mut object, 0x2c, CAM_DEMO_BLOCKER_PID as i32);
            }
            1 => world::word(&mut object, 0x74, 30),
            _ => {}
        }
        objects.push(object);
    }
    let count = objects.len() as i32;
    bytes.extend(world::words(&[count, count]));
    bytes.extend(objects.concat());
    bytes.extend(world::words(&[0, 0]));
    bytes
}

/// Writes the camera demo into `root`, a corpus [`write_corpus`] wrote:
/// `maps/cam_demo.map`, its empty `maps/cam_demo.gam`, and
/// `scripts/camdemo.int`, whose [`CAM_DEMO_SCRIPT`] row is appended to
/// `scripts/scripts.lst` (once). It adds files only; the map uses the
/// corpus's prototypes and art. Returns the script's list index.
pub fn write_cam_demo(root: &std::path::Path) -> i32 {
    for folder in ["maps", "scripts"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    let list = root.join("scripts/scripts.lst");
    let mut text = std::fs::read_to_string(&list).unwrap_or_default();
    let index = match text.lines().position(|line| line == CAM_DEMO_SCRIPT) {
        Some(index) => index,
        None => {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            let index = text.lines().count();
            text.push_str(CAM_DEMO_SCRIPT);
            text.push('\n');
            std::fs::write(&list, &text).unwrap();
            index
        }
    } as i32;
    std::fs::write(root.join("scripts/camdemo.int"), cam_demo_program()).unwrap();
    std::fs::write(root.join("maps/cam_demo.map"), cam_demo_map(index)).unwrap();
    std::fs::write(root.join("maps/cam_demo.gam"), b"MAP_GLOBAL_VARS:\n").unwrap();
    index
}

/// The camera demo's registered script row in [`write_cam_demo_mod`].
pub const CAM_DEMO_MOD_SCRIPT: u32 = 4096;
/// The camera demo's registered map number in [`write_cam_demo_mod`].
pub const CAM_DEMO_MOD_MAP: u32 = 100;

/// Where [`write_cam_demo_mod`] put the demo layer.
pub struct CamDemoMod {
    /// `root/load_order.toml`, naming the layer.
    pub load_order: std::path::PathBuf,
    /// `root/mods/cam-demo`.
    pub layer: std::path::PathBuf,
    pub script: u32,
    pub map: u32,
}

/// The camera demo as an additive mod layer over a corpus [`write_corpus`]
/// wrote in `root`: `root/mods/cam-demo` holds `scripts/camdemo.int`,
/// `maps/cam_demo.map` (map number [`CAM_DEMO_MOD_MAP`], its map script
/// row [`CAM_DEMO_MOD_SCRIPT`]) and its empty `.gam`, and a
/// `vault-mod.toml` that registers both and selects the enhanced preset.
/// `scripts.lst` is left as it is. `root/load_order.toml` names the layer.
pub fn write_cam_demo_mod(root: &std::path::Path) -> CamDemoMod {
    let layer = root.join("mods/cam-demo");
    for folder in ["maps", "scripts"] {
        std::fs::create_dir_all(layer.join(folder)).unwrap();
    }
    std::fs::write(layer.join("scripts/camdemo.int"), cam_demo_program()).unwrap();
    let mut map = cam_demo_map(CAM_DEMO_MOD_SCRIPT as i32);
    world::word(&mut map, 0x34, CAM_DEMO_MOD_MAP as i32);
    std::fs::write(layer.join("maps/cam_demo.map"), map).unwrap();
    std::fs::write(layer.join("maps/cam_demo.gam"), b"MAP_GLOBAL_VARS:\n").unwrap();
    std::fs::write(
        layer.join("vault-mod.toml"),
        format!(
            "name = \"Camera demo\"\nversion = \"1.0.0\"\n\n[enhanced]\npreset = \"enhanced\"\n\n\
             [[scripts]]\nfile = \"camdemo.int\"\nindex = {CAM_DEMO_MOD_SCRIPT}\nlocal_vars = 0\n\n\
             [[maps]]\nfile = \"cam_demo.map\"\nnumber = {CAM_DEMO_MOD_MAP}\n"
        ),
    )
    .unwrap();
    let load_order = root.join("load_order.toml");
    std::fs::write(&load_order, "layers = [\"mods/cam-demo\"]\n").unwrap();
    CamDemoMod {
        load_order,
        layer,
        script: CAM_DEMO_MOD_SCRIPT,
        map: CAM_DEMO_MOD_MAP,
    }
}
