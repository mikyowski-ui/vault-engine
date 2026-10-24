pub use crate::formats as assets;
pub use crate::world;

pub fn gcd() -> Vec<u8> {
    assets::sparse(include_str!("player.gcd.hex"))
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
