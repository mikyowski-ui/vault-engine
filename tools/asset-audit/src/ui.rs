//! SPEC-013 and SPEC-014 corpus checks: the message texts and indices the
//! interactions and the user interface use, interface art sizes and frame
//! counts, `intrface.lst` positions, cursor hotspots by the SPEC-001 rule,
//! color keys through `color.pal`, fonts, the elevator destinations, splash
//! images, credits, endgame slide resources, premades, the location table
//! and the new-game map.
use std::{collections::BTreeSet, error::Error};
use vault_core::{
    skills,
    ui::{ColorKey, cursor, endgame, flow, saveload},
};
use vault_formats::{
    Font, FrmArchive, LocalDirProvider, LocalPrototypeCatalog, LstKind, LstTable, MAX_ASSET_BYTES,
    MapFile, MsgCatalog, Palette, Vfs, credits, rix,
};

/// Pass and fail counts per category, failures reported on stderr.
#[derive(Debug, Default)]
pub struct UiAudit {
    categories: Vec<(&'static str, u64, u64)>,
}
impl UiAudit {
    fn record(&mut self, category: &'static str, label: &str, result: Result<(), String>) {
        let index = match self
            .categories
            .iter()
            .position(|(name, ..)| *name == category)
        {
            Some(index) => index,
            None => {
                self.categories.push((category, 0, 0));
                self.categories.len() - 1
            }
        };
        match result {
            Ok(()) => self.categories[index].1 += 1,
            Err(error) => {
                self.categories[index].2 += 1;
                eprintln!("FAIL {category}: {label}: {error}");
            }
        }
    }
    pub fn report(&self, title: &str) {
        for (category, passed, failed) in &self.categories {
            println!("{title} {category}: {passed} passed, {failed} failed");
        }
    }
    pub fn failed(&self) -> u64 {
        self.categories.iter().map(|(_, _, failed)| failed).sum()
    }
}

fn read(vfs: &LocalDirProvider, path: &str) -> Result<Vec<u8>, String> {
    vfs.read_bounded(path, MAX_ASSET_BYTES)
        .map_err(|error| error.to_string())
}
fn art(vfs: &LocalDirProvider, name: &str) -> Result<FrmArchive, String> {
    let path = if name.contains('/') {
        name.to_owned()
    } else {
        format!("art/intrface/{name}")
    };
    FrmArchive::parse(&read(vfs, &path)?).map_err(|error| error.to_string())
}
/// Frame 0's size and the frame count.
fn size(vfs: &LocalDirProvider, name: &str) -> Result<([u16; 2], usize), String> {
    let archive = art(vfs, name)?;
    let frame = archive.frame(0, 0).ok_or("no frame 0")?;
    Ok(([frame.width, frame.height], archive.frame_count()))
}
fn check_size(
    audit: &mut UiAudit,
    vfs: &LocalDirProvider,
    name: &str,
    expected: [u16; 2],
    frames: Option<usize>,
) {
    let result = size(vfs, name).and_then(|(found, count)| {
        if found != expected {
            return Err(format!(
                "{}x{} != {}x{}",
                found[0], found[1], expected[0], expected[1]
            ));
        }
        match frames {
            Some(frames) if frames != count => Err(format!("{count} frames != {frames}")),
            _ => Ok(()),
        }
    });
    audit.record("art", name, result);
}
fn catalog(vfs: &LocalDirProvider, file: &str) -> Result<MsgCatalog, String> {
    MsgCatalog::parse(&read(vfs, &format!("text/english/game/{file}"))?)
        .map_err(|error| error.to_string())
}
/// Exact message texts (`None` checks presence only).
fn check_messages(
    audit: &mut UiAudit,
    vfs: &LocalDirProvider,
    file: &str,
    expected: &[(i32, Option<&str>)],
) {
    let catalog = match catalog(vfs, file) {
        Ok(catalog) => catalog,
        Err(error) => return audit.record("messages", file, Err(error)),
    };
    for (id, text) in expected {
        let result = match (catalog.get(*id), text) {
            (None, _) => Err("missing".into()),
            (Some(message), Some(text)) if message.text != text.as_bytes() => Err(format!(
                "{:?} != {text:?}",
                String::from_utf8_lossy(&message.text)
            )),
            _ => Ok(()),
        };
        audit.record("messages", &format!("{file} {id}"), result);
    }
}

/// SPEC-013: message texts, window art, and elevator destinations.
pub fn audit_interactions(vfs: &LocalDirProvider) -> Result<UiAudit, Box<dyn Error>> {
    let mut audit = UiAudit::default();
    check_messages(
        &mut audit,
        vfs,
        "skill.msg",
        &[
            (500, Some("You heal %d hit points.")),
            (501, Some("You look healthy already.")),
            (502, Some("%s looks healthy already.")),
            (503, Some("You fail to do any healing.")),
            (505, Some("You earn %d XP for honing your skills.")),
            (510, None),
            (512, Some("You can't heal the dead.")),
            (513, Some("Let the dead rest in peace.")),
            (514, Some("It's dead, get over it.")),
            (520, Some("You heal your %s.")),
            (521, Some("You heal the %s.")),
            (525, Some("You fail to heal your %s.")),
            (526, Some("You fail to heal the %s.")),
            (530, Some("damaged eye")),
            (531, Some("crippled left arm")),
            (532, Some("crippled right arm")),
            (533, Some("crippled right leg")),
            (534, Some("crippled left leg")),
            (550, Some("You fail to pick the lock.")),
            (551, Some("You fail to find any traps.")),
            (552, Some("You fail to learn anything.")),
            (553, Some("You cannot repair that.")),
            (570, Some("You're caught stealing the %s.")),
            (571, Some("You steal the %s.")),
            (572, Some("You're caught planting the %s.")),
            (573, Some("You plant the %s.")),
            (
                590,
                Some("You've taxed your ability with that skill.  Wait a while."),
            ),
            (591, Some("You're too tired.")),
            (592, Some("The strain might kill you.")),
        ],
    );
    let skilldex: Vec<_> = [
        "SKILLDEX",
        "CANCEL",
        "SNEAK",
        "LOCKPICK",
        "STEAL",
        "TRAPS",
        "FIRST AID",
        "DOCTOR",
        "SCIENCE",
        "REPAIR",
    ]
    .iter()
    .enumerate()
    .map(|(i, text)| (100 + i as i32, Some(*text)))
    .collect();
    check_messages(&mut audit, vfs, "skilldex.msg", &skilldex);
    check_messages(
        &mut audit,
        vfs,
        "proto.msg",
        &[
            (480, Some("You see: %s.")),
            (485, Some("You search the %s.")),
            (486, Some("You close the %s.")),
            (487, Some("It is locked.")),
            (582, Some("That does nothing.")),
            (589, Some("You set the timer.")),
            (590, Some("The timer is already ticking!")),
            (700, Some("You don't have enough action points.")),
            (900, Some("The supplies in the Doctor's Bag run out.")),
            (901, Some("The supplies in the First Aid Kit run out.")),
            (902, Some("You cannot do that in combat.")),
            (903, Some("This person will not barter with you.")),
        ],
    );
    let mut misc: Vec<_> = (1000..=1005).map(|id| (id, None)).collect();
    misc.extend([
        (1006, Some("You have died from radiation sickness.")),
        (1007, Some("You have received a large dose of radiation.")),
        (1008, Some("The geiger counter is clicking.")),
        (1009, Some("The geiger counter is clicking wildly.")),
        (2000, Some("You cannot get there.")),
        (2001, Some("The lock is jammed.")),
        (3000, Some("You have been poisoned!")),
        (3001, Some("You take damage from poison.")),
        (
            4000,
            Some("Due to your inept handling, the explosive detonates prematurely."),
        ),
    ]);
    check_messages(&mut audit, vfs, "misc.msg", &misc);
    check_messages(
        &mut audit,
        vfs,
        "inventry.msg",
        &[
            (21, Some("MOVE ITEMS")),
            (22, Some("ALL")),
            (23, Some("SET TIMER")),
            (
                25,
                Some("You cannot pick that up. You are at your maximum weight capacity."),
            ),
            (26, Some("There is no space left for that item.")),
            (27, None),
            (28, None),
            (
                29,
                Some("You gain %d experience points for successfully using your Steal skill."),
            ),
        ],
    );
    // The inventory catalog ends at 29, so the Take All refusal (31) is silent.
    let highest = catalog(vfs, "inventry.msg").map(|c| c.entries().keys().max().copied());
    audit.record(
        "messages",
        "inventry.msg holds 0-29 only",
        match highest {
            Ok(Some(29)) => Ok(()),
            other => Err(format!("highest index {other:?}")),
        },
    );

    for (name, expected) in [
        ("el_bos.frm", [230, 284]),
        ("el_mast1.frm", [230, 284]),
        ("el_mil1.frm", [231, 285]),
        ("el_vault.frm", [230, 284]),
        ("el_bos2.frm", [107, 248]),
        ("el_mast2.frm", [107, 247]),
        ("el_mil2.frm", [107, 248]),
        ("el_mil3.frm", [107, 248]),
        ("el_mil4.frm", [107, 248]),
        ("ebut_out.frm", [48, 50]),
        ("ebut_in.frm", [48, 50]),
        ("gaj000.frm", [92, 715]),
        ("skldxbox.frm", [185, 368]),
        ("skldxoff.frm", [86, 33]),
        ("skldxon.frm", [86, 33]),
        ("lilredup.frm", [15, 16]),
        ("lilreddn.frm", [15, 16]),
        ("bignum.frm", [336, 24]),
        ("movemult.frm", [259, 162]),
        ("timer.frm", [105, 81]),
        ("splsoff.frm", [16, 12]),
        ("splson.frm", [16, 12]),
        ("snegoff.frm", [17, 12]),
        ("snegon.frm", [17, 12]),
        ("allbon.frm", [94, 33]),
        ("allboff.frm", [94, 33]),
        ("loot.frm", [537, 376]),
        ("invupout.frm", [22, 23]),
        ("invupin.frm", [22, 23]),
        ("invdnout.frm", [22, 23]),
        ("invdnin.frm", [22, 23]),
        ("slu.frm", [20, 18]),
        ("sld.frm", [20, 18]),
        ("sru.frm", [20, 18]),
        ("srd.frm", [20, 18]),
        ("barter.frm", [640, 191]),
        ("di_rdbt1.frm", [14, 14]),
        ("di_rdbt2.frm", [14, 14]),
        ("di_up1.frm", [23, 24]),
        ("di_up2.frm", [23, 24]),
        ("di_down1.frm", [24, 25]),
        ("di_down2.frm", [24, 25]),
    ] {
        check_size(&mut audit, vfs, name, expected, None);
    }
    // Elevator destinations: an existing map number, hex and elevation.
    let prototypes = LocalPrototypeCatalog::new(vfs);
    let numbers: BTreeSet<i32> = vfs
        .files()
        .filter(|path| path.extension() == Some("map"))
        .filter_map(|path| read(vfs, path.as_str()).ok())
        .filter_map(|bytes| MapFile::parse(&bytes, &prototypes).ok())
        .map(|map| map.header.map_number)
        .collect();
    for (kind, elevator) in skills::ELEVATORS.iter().enumerate() {
        for (button, (map, elevation, hex)) in elevator.destinations.iter().enumerate() {
            let result = if !numbers.contains(map) {
                Err(format!("map {map} is not in the corpus"))
            } else if *elevation > 2 || !(0..40000).contains(hex) {
                Err(format!("elevation {elevation}, hex {hex}"))
            } else {
                Ok(())
            };
            audit.record("elevators", &format!("type {kind} button {button}"), result);
        }
        for art_name in std::iter::once(elevator.background).chain(elevator.panel) {
            audit.record(
                "elevators",
                &format!("type {kind} {art_name}"),
                art(vfs, art_name).map(|_| ()),
            );
        }
    }
    Ok(audit)
}

/// SPEC-014: interface art, list positions, cursors, colors, fonts,
/// messages, splash images, credits, endgame slides, premades, locations and
/// the new-game map.
pub fn audit_ui(vfs: &LocalDirProvider) -> Result<UiAudit, Box<dyn Error>> {
    let mut audit = UiAudit::default();
    // intrface.lst positions (zero-based).
    let list = read(vfs, "art/intrface/intrface.lst")
        .and_then(|bytes| LstTable::parse(&bytes, LstKind::Art).map_err(|e| e.to_string()));
    for (index, name) in [
        (16, "iface.frm"),
        (140, "mainmenu.frm"),
        (169, "edtrcrte.frm"),
        (170, "bignum.frm"),
        (174, "pickchar.frm"),
        (201, "combat.frm"),
        (202, "stealth.frm"),
        (203, "diplomat.frm"),
        (299, "menuup.frm"),
        (300, "menudown.frm"),
        (309, "death.frm"),
    ] {
        let result = list.as_ref().map_err(Clone::clone).and_then(|list| {
            let entry = list.entries().get(index).ok_or("missing entry")?;
            let found = entry
                .name
                .as_deref()
                .map(|n| String::from_utf8_lossy(n).to_ascii_lowercase());
            (found.as_deref() == Some(name))
                .then_some(())
                .ok_or(format!("{found:?}"))
        });
        audit.record("list", &format!("{index} {name}"), result);
    }
    for (name, expected, frames) in [
        ("mainmenu.frm", [640, 480], None),
        ("menuup.frm", [26, 26], None),
        ("menudown.frm", [26, 26], None),
        ("combat.frm", [212, 187], None),
        ("stealth.frm", [212, 187], None),
        ("diplomat.frm", [212, 187], None),
        ("bignum.frm", [336, 24], None),
        ("tgskloff.frm", [17, 11], None),
        ("tgsklon.frm", [17, 11], None),
        ("charwin.frm", [139, 72], None),
        ("donebox.frm", [108, 24], None),
        ("namebox.frm", [111, 20], None),
        ("agebox.frm", [124, 29], None),
        ("maleoff.frm", [45, 43], None),
        ("femoff.frm", [45, 42], None),
        ("opbase.frm", [164, 217], None),
        ("opbtnoff.frm", [137, 34], None),
        ("opbtnon.frm", [137, 34], None),
        ("lgdialog.frm", [302, 127], None),
        ("medialog.frm", [215, 113], None),
        ("lsgame.frm", [640, 480], None),
        ("lscover.frm", [275, 173], None),
        ("lsgbox.frm", [290, 85], None),
        ("uparwoff.frm", [11, 14], None),
        ("dnarwoff.frm", [11, 14], None),
        ("prfbknbs.frm", [46, 188], None),
        ("prflknbs.frm", [22, 50], None),
        ("prfsldof.frm", [21, 12], None),
        ("prfsldon.frm", [21, 12], None),
        ("prfxout.frm", [18, 18], None),
        ("prfxin.frm", [18, 18], None),
        ("prefcvr.frm", [207, 89], None),
        ("invbutup.frm", [32, 21], None),
        ("optiup.frm", [34, 34], None),
        ("bigredup.frm", [22, 21], None),
        ("mapup.frm", [41, 19], None),
        ("chaup.frm", [41, 19], None),
        ("pipup.frm", [41, 19], None),
        ("sattkbup.frm", [188, 67], None),
        ("saturbup.frm", [188, 67], None),
        ("endturnu.frm", [38, 22], None),
        ("endcmbtu.frm", [38, 22], None),
        ("numbers.frm", [360, 17], None),
        ("hlgrn.frm", [5, 5], None),
        ("hlred.frm", [5, 5], None),
        ("hlyel.frm", [5, 5], None),
        ("bullseye.frm", [18, 18], None),
        ("mvepnt.frm", [17, 12], None),
        ("warnbox.frm", [130, 21], None),
        ("endanim.frm", [57, 58], Some(5)),
        ("endltgrn.frm", [57, 58], None),
        ("endltred.frm", [57, 58], None),
        ("actpick.frm", [69, 62], None),
        ("actmenu.frm", [69, 222], None),
        ("acttohit.frm", [50, 21], None),
        ("actarrow.frm", [29, 23], None),
        ("msef003.frm", [32, 16], None),
        ("death.frm", [640, 480], None),
        ("dp.frm", [1280, 480], None),
        ("tblcrd.frm", [369, 279], Some(31)),
    ] {
        check_size(&mut audit, vfs, name, expected, frames);
    }
    for entry in [
        cursor::Entry::Use,
        cursor::Entry::Look,
        cursor::Entry::Inventory,
        cursor::Entry::Skill,
        cursor::Entry::Rotate,
        cursor::Entry::Talk,
        cursor::Entry::Cancel,
    ] {
        let (normal, highlighted) = entry.icons();
        check_size(&mut audit, vfs, normal, [40, 40], None);
        check_size(&mut audit, vfs, highlighted, [40, 40], None);
    }
    // Cursors: size, and the hotspot by the SPEC-001 rule.
    let cursors = [
        cursor::ARROW,
        cursor::BLANK,
        cursor::SMALL_UP,
        cursor::SMALL_DOWN,
        cursor::CROSSHAIR,
        cursor::PLUS,
        cursor::DESTROY,
        cursor::USE_CROSSHAIR,
        cursor::WATCH,
        cursor::PLANET,
        cursor::ANIMATED_WATCH,
    ]
    .into_iter()
    .chain(cursor::SCROLL)
    .chain(cursor::SCROLL_BLOCKED);
    for shape in cursors {
        let result = art(vfs, shape.art).and_then(|archive| {
            let frame = archive.frame(0, 0).ok_or("no frame 0")?;
            let size = [i32::from(frame.width), i32::from(frame.height)];
            if size != shape.size {
                return Err(format!("{size:?} != {:?}", shape.size));
            }
            match shape.hotspot {
                Some(expected) if frame.cursor_hotspot(archive.shifts[0]) != expected => {
                    Err(format!(
                        "hotspot {:?} != {expected:?}",
                        frame.cursor_hotspot(archive.shifts[0])
                    ))
                }
                _ => Ok(()),
            }
        });
        audit.record("cursors", shape.art, result);
    }
    for (name, frames) in cursor::ANIMATED {
        let result = art(vfs, name).and_then(|archive| {
            (archive.frame_count() == frames as usize)
                .then_some(())
                .ok_or(format!("{} frames", archive.frame_count()))
        });
        audit.record("cursors", &format!("{name} frames"), result);
    }
    // Named color keys resolve to the documented palette indexes.
    let palette =
        read(vfs, "color.pal").and_then(|bytes| Palette::parse(&bytes).map_err(|e| e.to_string()));
    for (key, index) in [
        (ColorKey::LABEL, 62),
        (ColorKey::LABEL_PRESSED, 63),
        (ColorKey::GREEN, 215),
        (ColorKey::SELECTED, 57),
        (ColorKey::ERROR, 145),
        (ColorKey::AMBER, 60),
        (ColorKey::GREY_GREEN, 85),
        (ColorKey::WHITE, 220),
        (ColorKey::GREY, 6),
        (ColorKey::DARK_GREEN, 96),
        (ColorKey::DARK_OLIVE, 80),
        (ColorKey::BLACK, 228),
        (endgame::GREY_NAME, 201),
        (ColorKey(13673), 76),
        (ColorKey(18917), 62),
    ] {
        let result = palette.as_ref().map_err(Clone::clone).and_then(|palette| {
            let found = key.resolve(palette);
            (found == index)
                .then_some(())
                .ok_or(format!("{found} != {index}"))
        });
        audit.record("colors", &format!("key {}", key.0), result);
    }
    // Fonts 100-104: font0.aaf through font4.aaf line heights.
    for (number, height) in [12, 10, 28, 20, 26].into_iter().enumerate() {
        let path = format!("font{number}.aaf");
        let result = read(vfs, &path)
            .and_then(|bytes| Font::parse_aaf(&bytes).map_err(|e| e.to_string()))
            .and_then(|font| {
                (font.line_height() == height)
                    .then_some(())
                    .ok_or(format!("line height {}", font.line_height()))
            });
        audit.record("fonts", &path, result);
    }
    let mut misc = vec![
        (0, Some("Are you sure you want to quit?")),
        (4, Some("Quick load game successfully loaded.")),
        (5, Some("Quick save game successfully saved.")),
        (7, Some("Pipboy not available in combat!")),
        (14, None),
        (15, Some("Action Points")),
        (16, Some("Hit Points")),
    ];
    for (i, text) in ["INTRO", "NEW GAME", "LOAD GAME", "CREDITS", "EXIT"]
        .iter()
        .enumerate()
    {
        misc.push((9 + i as i32, Some(text)));
    }
    check_messages(&mut audit, vfs, "misc.msg", &misc);
    let mut lsgame: Vec<_> = (100..=103)
        .chain([106, 107, 108, 131, 132, 133, 134, 135])
        .map(|id| (id, None))
        .collect();
    lsgame.extend([
        (104, Some("DONE")),
        (105, Some("CANCEL")),
        (109, Some("SAVE GAME")),
        (110, Some("LOAD GAME")),
        (111, Some("- EMPTY -")),
        (112, Some("- CORRUPT SAVE  FILE -")),
        (113, Some("- OLD VERSION -")),
        (114, Some("Empty.")),
        (115, Some("Error!")),
        (116, Some("Old version.")),
        (130, Some("DESCRIPTION")),
        (136, Some("Save game file is an older version.")),
        (140, Some("Game Saved.")),
        (141, Some("Game Loaded.")),
    ]);
    for (i, month) in [
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ]
    .iter()
    .enumerate()
    {
        lsgame.push((117 + i as i32, Some(month)));
    }
    check_messages(&mut audit, vfs, "lsgame.msg", &lsgame);
    let mut options: Vec<_> = (0..=4)
        .chain(101..=122)
        .chain(201..=219)
        .chain([221, 222, 223])
        .map(|id| (id, None))
        .collect();
    options.extend([
        (100, Some("GAME PREFERENCES")),
        (300, Some("DONE")),
        (301, Some("PAUSED")),
    ]);
    check_messages(&mut audit, vfs, "options.msg", &options);
    check_messages(
        &mut audit,
        vfs,
        "dbox.msg",
        &[(100, Some("DONE")), (101, Some("YES")), (102, Some("NO"))],
    );
    check_messages(
        &mut audit,
        vfs,
        "intrface.msg",
        &[
            (100, Some("SNEAK")),
            (101, Some("LEVEL")),
            (102, Some("CHEM")),
        ],
    );
    let editor: Vec<_> = [
        100, 101, 102, 104, 107, 108, 116, 117, 118, 119, 138, 139, 140, 141, 142, 143, 148, 149,
        157, 158,
    ]
    .into_iter()
    .chain(300..=318)
    .chain(500..=511)
    .chain(600..=607)
    .chain([609, 610, 612, 615, 616, 617])
    .map(|id| (id, None))
    .collect();
    check_messages(&mut audit, vfs, "editor.msg", &editor);
    // Location short names and the per-map table.
    let map_names: Vec<_> = saveload::MAP_SHORT_NAMES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|id| (id, None))
        .collect();
    check_messages(&mut audit, vfs, "map.msg", &map_names);
    // Splash images.
    for path in vfs
        .files()
        .filter(|path| path.as_str().starts_with("art/splash/"))
    {
        let result = read(vfs, path.as_str()).and_then(|bytes| {
            if bytes.len() != rix::FILE_LEN {
                return Err(format!("{} bytes", bytes.len()));
            }
            rix::Splash::parse(&bytes)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
        audit.record("splash", path.as_str(), result);
    }
    // Credits and quotes.
    for file in ["credits.txt", "quotes.txt"] {
        let path = format!("text/english/{file}");
        let result = read(vfs, &path).and_then(|bytes| {
            let rows = credits::parse_credits(&bytes);
            (!rows.is_empty()).then_some(()).ok_or("no rows".into())
        });
        audit.record("credits", &path, result);
    }
    // Endgame slides for every reachable variant: art, palette, narration and
    // subtitles.
    let mut slides = BTreeSet::new();
    for globals in endgame_variants() {
        for slide in endgame::slides(&globals) {
            slides.insert((slide.art, slide.narration));
        }
    }
    for (art_name, narration) in slides {
        let slide = endgame::Slide {
            art: art_name,
            narration,
            pan: None,
        };
        let result = art(vfs, &slide.art_path())
            .and_then(|_| read(vfs, &slide.palette_path()))
            .and_then(|bytes| {
                Palette::parse(&bytes)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .and_then(|_| read(vfs, &slide.narration_path()).map(|_| ()))
            .and_then(|_| read(vfs, &slide.subtitle_path("english")).map(|_| ()));
        audit.record("endgame", &format!("{art_name} {narration}"), result);
    }
    // Premade characters.
    for premade in flow::PREMADES {
        let result = read(vfs, premade.sheet)
            .and_then(|bytes| {
                vault_formats::GcdFile::parse(&bytes)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .and_then(|_| read(vfs, premade.bio).map(|_| ()))
            .and_then(|_| art(vfs, premade.portrait).map(|_| ()));
        audit.record("premades", premade.sheet, result);
    }
    // The new-game map header.
    let prototypes = LocalPrototypeCatalog::new(vfs);
    let result = read(vfs, flow::NEW_GAME_MAP)
        .and_then(|bytes| MapFile::parse(&bytes, &prototypes).map_err(|e| e.to_string()))
        .and_then(|map| {
            let header = &map.header;
            let found = (
                header.entry_hex,
                header.entry_elevation,
                header.entry_orientation,
            );
            (found == (20100, 0, 0))
                .then_some(())
                .ok_or(format!("entry {found:?}"))
        });
    audit.record("new game", flow::NEW_GAME_MAP, result);
    for path in ["art/skilldex", "art/splash"] {
        let found = vfs.files().any(|file| file.as_str().starts_with(path));
        audit.record(
            "directories",
            path,
            found.then_some(()).ok_or("missing".into()),
        );
    }
    Ok(audit)
}

/// Global settings reaching every endgame slide branch.
fn endgame_variants() -> Vec<Vec<i32>> {
    let settings: &[&[(usize, i32)]] = &[
        &[],
        &[
            (51, 1),
            (13, 1),
            (7, 1),
            (12, 1),
            (15, 1),
            (108, 2),
            (250, 1),
            (14, 1),
        ],
        &[
            (30, 1),
            (31, 2),
            (132, 1),
            (604, 1),
            (26, 1),
            (606, 1),
            (69, 2),
            (115, 6),
        ],
        &[(30, 1), (604, 1), (104, 2), (69, 2), (114, 1), (115, 7)],
        &[(26, 1)],
    ];
    settings
        .iter()
        .map(|values| {
            let mut globals = vec![0; 700];
            for (index, value) in *values {
                globals[*index] = *value;
            }
            globals
        })
        .collect()
}
