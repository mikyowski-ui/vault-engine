//! SPEC-007 §4.7 Fixture G: the §3.10.1 start rule over scheduler passes.
//! Every animation and the projectile run at 10 fps, so each running entry
//! takes one frame step per pass; pass k is at `100 * k` ms and pass 0 is
//! registration.
use crate::choreography::{Action, Display, Frames, Owner, Sequence, Started};

const ATTACKER: Owner = Owner::World(1);
const DEFENDER: Owner = Owner::World(2);
const PROJECTILE: Owner = Owner::World(3);

fn frames(count: usize) -> Frames {
    Frames {
        shifts: vec![[0, 0]; count],
        interval_ms: 100,
    }
}
fn animate(count: usize) -> Action {
    Action::Animate {
        art: 0x0100_0000,
        frames: frames(count),
        hide: false,
    }
}
fn sound(name: &str) -> Action {
    Action::Sound {
        name: name.as_bytes().to_vec(),
        object: None,
    }
}
fn shown() -> Display {
    Display {
        pid: 0x0100_0001,
        art: 0x0100_0000,
        frame: 0,
        hex: Some(20100),
        elevation: 0,
        orientation: 0,
        displacement: [0; 2],
        hidden: false,
        flat: false,
    }
}
/// A sequence over the three owners from `(owner, delay, action)` rows.
fn sequence(rows: Vec<(Owner, i32, Action)>) -> Sequence {
    let mut sequence = Sequence::default();
    for owner in [ATTACKER, DEFENDER, PROJECTILE] {
        sequence.show(owner, shown());
    }
    for (owner, delay, action) in rows {
        sequence.push(owner, delay, action).unwrap();
    }
    sequence
}
fn names(started: &[Started]) -> Vec<&[u8]> {
    started
        .iter()
        .filter_map(|item| match item {
            Started::Sound { name, .. } => Some(name.as_slice()),
            _ => None,
        })
        .collect()
}
fn frame(sequence: &Sequence, owner: Owner) -> usize {
    sequence.display(owner).unwrap().frame
}

#[test]
fn spec007_fixture_g1_punch_hit() {
    // Turn, attack sound, attack animation (11 frames, A = 3), contact
    // sound, flinch sound and flinch animation.
    let mut punch = sequence(vec![
        (
            ATTACKER,
            0,
            Action::Turn {
                orientation: 2,
                art: 0x0100_0000,
            },
        ),
        (ATTACKER, 0, sound("swing")),
        (ATTACKER, 0, animate(11)),
        (ATTACKER, 3, sound("contact")),
        (DEFENDER, 0, sound("flinch")),
        (DEFENDER, 0, animate(5)),
        (ATTACKER, -1, Action::Taunt(0)),
    ]);
    assert_eq!(names(&punch.register(0)), [b"swing".as_slice()]);
    assert!(punch.running(2) && !punch.started(3));
    for pass in 1..=2 {
        assert!(names(&punch.pass(100 * pass)).is_empty());
        assert_eq!(frame(&punch, ATTACKER), pass as usize - 1);
    }
    let started = punch.pass(300);
    assert_eq!(
        names(&started),
        [b"contact".as_slice(), b"flinch".as_slice()]
    );
    assert_eq!(frame(&punch, ATTACKER), 2, "A - 1");
    assert!(punch.running(5));
    assert_eq!(frame(&punch, DEFENDER), 0, "the flinch steps in its pass");
}

#[test]
fn spec007_fixture_g2_g3_dodges_line_up_the_action_frames() {
    // G2: A = 3, dodge B = 1: the dodge waits A - B = 2.
    let mut early = sequence(vec![
        (
            ATTACKER,
            0,
            Action::Turn {
                orientation: 2,
                art: 0x0100_0000,
            },
        ),
        (ATTACKER, -1, sound("swing")),
        (ATTACKER, 0, animate(11)),
        (DEFENDER, 2, sound("dodge")),
        (DEFENDER, 0, animate(8)),
        (ATTACKER, -1, Action::Taunt(0)),
    ]);
    assert_eq!(names(&early.register(0)), [b"swing".as_slice()]);
    assert!(names(&early.pass(100)).is_empty());
    assert_eq!(names(&early.pass(200)), [b"dodge".as_slice()]);
    assert!(early.running(4));
    assert_eq!(frame(&early, ATTACKER), 1);
    // G3: B = 5 > A: the attack waits B - A = 2 behind the dodge.
    let mut late = sequence(vec![
        (
            ATTACKER,
            0,
            Action::Turn {
                orientation: 2,
                art: 0x0100_0000,
            },
        ),
        (DEFENDER, -1, sound("dodge")),
        (DEFENDER, 0, animate(8)),
        (ATTACKER, 2, sound("swing")),
        (ATTACKER, 0, animate(11)),
        (ATTACKER, -1, Action::Taunt(0)),
    ]);
    assert_eq!(names(&late.register(0)), [b"dodge".as_slice()]);
    assert!(names(&late.pass(100)).is_empty());
    assert_eq!(names(&late.pass(200)), [b"swing".as_slice()]);
    assert_eq!(frame(&late, ATTACKER), 0, "attack frame 0 in the same pass");
    assert!(late.running(4));
}

/// G4-G6: a thrown knife (throw art `throw` frames, A = 4) whose flight has
/// `nodes` nodes and hit-sound delay `n`; the defender reacts to a hit.
fn knife(throw: usize, nodes: usize, n: i32) -> Sequence {
    let path = (0..nodes)
        .map(|node| (20300 + node as i32, [0, 0]))
        .collect();
    let mut knife = sequence(vec![
        (
            ATTACKER,
            0,
            Action::Turn {
                orientation: 2,
                art: 0x0100_0000,
            },
        ),
        (ATTACKER, -1, sound("throw")),
        (ATTACKER, 0, animate(throw)),
        (PROJECTILE, 4, Action::Unhide),
        (PROJECTILE, 0, sound("flight")),
        (
            PROJECTILE,
            0,
            Action::Move {
                art: 0x0500_0004,
                frames: frames(1),
                nodes: path,
            },
        ),
        (PROJECTILE, n, sound("hit")),
        (DEFENDER, 0, sound("flinch")),
        (DEFENDER, 0, animate(5)),
        (DEFENDER, -1, Action::Taunt(0)),
        (PROJECTILE, -1, Action::SetArt { art: 0x0000_0004 }),
    ]);
    knife.register(0);
    knife
}
/// G6: the same throw missing with nothing struck: no reaction.
fn missed_knife() -> Sequence {
    let path = (0..7).map(|node| (20300 + node, [0, 0])).collect();
    let mut knife = sequence(vec![
        (
            ATTACKER,
            0,
            Action::Turn {
                orientation: 2,
                art: 0x0100_0000,
            },
        ),
        (ATTACKER, -1, sound("throw")),
        (ATTACKER, 0, animate(12)),
        (PROJECTILE, 4, Action::Unhide),
        (PROJECTILE, 0, sound("flight")),
        (
            PROJECTILE,
            0,
            Action::Move {
                art: 0x0500_0004,
                frames: frames(1),
                nodes: path,
            },
        ),
        (PROJECTILE, 0, sound("hit")),
        (DEFENDER, -1, Action::Taunt(0)),
    ]);
    knife.register(0);
    knife
}
/// The node the projectile shows.
fn node(sequence: &Sequence) -> i32 {
    sequence.display(PROJECTILE).unwrap().hex.unwrap() - 20300
}

#[test]
fn spec007_fixture_g4_to_g6_thrown_knife_timing() {
    // G4: L = 200, so N = 6 over 7 nodes.
    let mut hit = knife(12, 7, 6);
    for pass in 1..=3 {
        assert!(names(&hit.pass(100 * pass)).is_empty());
    }
    let started = hit.pass(400);
    assert_eq!(names(&started), [b"flight".as_slice()]);
    assert_eq!((frame(&hit, ATTACKER), node(&hit)), (3, 0));
    assert!(!hit.display(PROJECTILE).unwrap().hidden);
    for pass in 5..=6 {
        assert!(names(&hit.pass(100 * pass)).is_empty());
    }
    let started = hit.pass(700);
    assert_eq!(names(&started), [b"hit".as_slice(), b"flinch".as_slice()]);
    assert!(hit.running(8));
    assert_eq!(node(&hit), 3);
    for pass in 8..=9 {
        hit.pass(100 * pass);
    }
    assert!(hit.running(5));
    hit.pass(1000);
    assert_eq!(node(&hit), 6);
    assert!(hit.started(5) && !hit.running(5), "the flight finishes");

    // G5: a 4-frame throw finishes in pass 5, examining twice; the hit
    // sound still starts in pass 7, at node 3.
    let mut short = knife(4, 7, 6);
    for pass in 1..=6 {
        assert!(!names(&short.pass(100 * pass)).contains(&b"hit".as_slice()));
    }
    assert!(!short.running(2), "the throw has finished");
    assert!(names(&short.pass(700)).contains(&b"hit".as_slice()));
    assert_eq!(node(&short), 3);

    // G6: a miss with nothing struck (N = 0): the hit sound starts with the
    // flight.
    let mut miss = missed_knife();
    for pass in 1..=3 {
        miss.pass(100 * pass);
    }
    let started = miss.pass(400);
    assert_eq!(names(&started), [b"flight".as_slice(), b"hit".as_slice()]);
}

#[test]
fn spec007_fixture_g7_laser_hit() {
    // Point (4 frames), single fire (5 frames, A = 1), a 3-node flight;
    // the non-explosive projectile is erased, then the hit sound and the
    // reaction.
    let path = (0..3).map(|node| (20300 + node, [0, 0])).collect();
    let mut laser = sequence(vec![
        (
            ATTACKER,
            0,
            Action::Turn {
                orientation: 2,
                art: 0x0100_0000,
            },
        ),
        (ATTACKER, -1, animate(4)),
        (ATTACKER, 0, Action::Taunt(0)),
        (ATTACKER, -1, sound("fire")),
        (ATTACKER, 0, animate(5)),
        (PROJECTILE, 1, Action::Unhide),
        (PROJECTILE, 0, sound("flight")),
        (
            PROJECTILE,
            0,
            Action::Move {
                art: 0x0500_0005,
                frames: frames(1),
                nodes: path,
            },
        ),
        (PROJECTILE, -1, Action::Remove),
        (ATTACKER, 2, sound("hit")),
        (DEFENDER, 0, sound("flinch")),
        (DEFENDER, 0, animate(5)),
        (DEFENDER, -1, Action::Taunt(1)),
        (ATTACKER, -1, animate(4)),
    ]);
    let started = laser.register(0);
    assert!(
        laser.running(1) && laser.started(2),
        "the taunt starts with the point"
    );
    assert!(names(&started).is_empty());
    for pass in 1..=4 {
        assert!(names(&laser.pass(100 * pass)).is_empty());
    }
    let started = laser.pass(500);
    assert!(!laser.running(1), "the point finishes");
    assert_eq!(names(&started), [b"fire".as_slice(), b"flight".as_slice()]);
    assert_eq!((frame(&laser, ATTACKER), node_of(&laser)), (0, 0));
    laser.pass(600);
    laser.pass(700);
    assert!(laser.started(7) && !laser.running(7), "the flight finishes");
    assert!(!laser.started(8));
    laser.pass(800);
    laser.pass(900);
    let started = laser.pass(1000);
    assert!(!laser.running(4), "the attack animation finishes");
    assert_eq!(laser.display(PROJECTILE).unwrap().hex, None, "erased");
    assert_eq!(names(&started), [b"hit".as_slice(), b"flinch".as_slice()]);
}
fn node_of(sequence: &Sequence) -> i32 {
    sequence.display(PROJECTILE).unwrap().hex.unwrap() - 20300
}

#[test]
fn spec007_start_rule_positive_delays_never_outlast_the_earlier_entries() {
    // Nothing earlier runs: a positive delay is ignored and the entry
    // starts at once; a capacity of 40 entries.
    let mut sequence = sequence(vec![
        (ATTACKER, 0, sound("first")),
        (ATTACKER, 9, sound("second")),
    ]);
    assert_eq!(
        names(&sequence.register(0)),
        [b"first".as_slice(), b"second".as_slice()]
    );
    assert!(sequence.finished());
    let mut full = Sequence::default();
    for _ in 0..crate::choreography::CAPACITY {
        full.push(ATTACKER, 0, Action::Hide).unwrap();
    }
    assert_eq!(
        full.push(ATTACKER, 0, Action::Hide),
        Err(crate::choreography::Refused::Full)
    );
}
