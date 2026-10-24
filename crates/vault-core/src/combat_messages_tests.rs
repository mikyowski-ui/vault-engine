//! SPEC-007 §3.12 combat message lines and §4.10 Fixture J.
use crate::combat_messages::{AttackLines, Named, Struck, lines, status_list};
use crate::critical::{effect, location};
use vault_formats::MsgCatalog;

/// The English `combat.msg` entries these tests read.
const COMBAT: &str = "\
{107}{}{You are not strong enough to use this weapon properly.}
{108}{}{ and }
{200}{}{knocked out}
{201}{}{knocked down}
{207}{}{were killed}
{215}{}{lost your next turn}
{257}{}{were killed}
{301}{}{knocked down}
{302}{}{crippled his left leg}
{307}{}{was killed}
{311}{}{had his armor bypassed}
{315}{}{lost his next turn}
{401}{}{knocked down}
{407}{}{was killed}
{506}{}{You}
{509}{}{Oops! %s were hit instead of %s!}
{513}{}{%s were hit for %d hit points}
{514}{}{%s critically missed}
{515}{}{%s missed}
{520}{}{%s were critically hit for %d hit points}
{523}{}{%s were hit for 1 hit point}
{527}{}{%s were hit for no damage}
{533}{}{%s critically missed and took 1 hit point}
{534}{}{%s critically missed and took %d hit points}
{608}{}{Oops! %s was hit instead of you!}
{609}{}{Oops! %s was hit instead of %s!}
{611}{}{%s was critically hit in %s for %d hit points}
{612}{}{%s was hit in %s for %d hit points}
{613}{}{%s was hit for %d hit points}
{615}{}{%s missed}
{620}{}{%s was critically hit for %d hit points}
{622}{}{%s was hit in %s for 1 hit point}
{623}{}{%s was hit for 1 hit point}
{626}{}{%s was hit in %s for no damage}
{627}{}{%s was hit for no damage}
{628}{}{%s was critically hit for no damage}
{713}{}{%s was hit for %d hit points}
{1110}{}{the head}
{1114}{}{the right leg}
{5004}{}{. The blow knocks him to the ground}
{5007}{}{, death is instantaneous}
";

fn catalog(text: &[u8]) -> MsgCatalog {
    MsgCatalog::parse(text).unwrap()
}
fn run(catalog: &MsgCatalog, attack: &AttackLines, verbose: bool) -> Vec<String> {
    lines(attack, verbose, &|id| {
        catalog.get(id).map(|m| m.text.clone())
    })
    .into_iter()
    .map(|line| String::from_utf8(line).unwrap())
    .collect()
}
fn you() -> Named {
    Named {
        name: b"You".to_vec(),
        player: true,
        ..Named::default()
    }
}
/// Fixture J's male NPC: art alias 11, so the head is 1110 "the head".
fn raider() -> Named {
    Named {
        name: b"Raider".to_vec(),
        alias: Some(11),
        ..Named::default()
    }
}
/// The player strikes the Raider in the head.
fn head_hit(damage: i32, flags: u32, critical: Option<i32>) -> AttackLines {
    AttackLines {
        attacker: Some(you()),
        aimed: Some(raider()),
        struck: Some(Struck::Critter(raider())),
        location: location::HEAD,
        damage,
        flags: flags
            | effect::HIT
            | if critical.is_some() {
                effect::CRITICAL
            } else {
                0
            },
        critical: critical.is_some(),
        critical_message: critical,
        ..AttackLines::default()
    }
}
/// Fixture J's rows: the attack and its Verbose and Brief lines (the
/// `[May-improve]` safe-rule texts where the reference prints a bare
/// " and ").
fn fixture_j() -> Vec<(AttackLines, Vec<&'static str>, Vec<&'static str>)> {
    vec![
        (
            head_hit(18, 0x802, Some(5004)),
            vec![
                "Raider was critically hit in the head for 18 hit points. The blow knocks him to the ground.",
            ],
            vec![
                "Raider was critically hit in the head for 18 hit points, knocked down and had his armor bypassed.",
            ],
        ),
        (
            head_hit(40, 0x80, Some(5007)),
            vec![
                "Raider was critically hit in the head for 40 hit points, death is instantaneous.",
                "Raider was killed.",
            ],
            vec!["Raider was critically hit in the head for 40 hit points and was killed."],
        ),
        (
            head_hit(5, 0, None),
            vec!["Raider was hit in the head for 5 hit points."],
            vec!["Raider was hit in the head for 5 hit points."],
        ),
        (
            AttackLines {
                location: location::TORSO,
                ..head_hit(1, 0, None)
            },
            vec!["Raider was hit for 1 hit point."],
            vec!["Raider was hit for 1 hit point."],
        ),
        (
            AttackLines {
                attacker: Some(you()),
                aimed: Some(raider()),
                critical_miss: true,
                attacker_damage: 3,
                attacker_flags: effect::CRITICAL | effect::LOSE_TURN,
                ..AttackLines::default()
            },
            vec!["You critically missed and took 3 hit points and lost your next turn."],
            vec!["You critically missed and took 3 hit points and lost your next turn."],
        ),
    ]
}

#[test]
fn spec007_fixture_j_combat_message_lines() {
    let catalog = catalog(COMBAT.as_bytes());
    for (attack, verbose, brief) in fixture_j() {
        assert_eq!(run(&catalog, &attack, true), verbose);
        assert_eq!(run(&catalog, &attack, false), brief);
    }
}

#[test]
#[ignore = "needs the local corpus"]
fn spec007_fixture_j_over_the_corpus_combat_msg() {
    let dir = std::env::var_os("VAULT_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../local-corpus")
        });
    let path = dir.join("fallout1-unpacked/text/english/game/combat.msg");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skipping: no corpus combat.msg at {}", path.display());
        return;
    };
    let catalog = catalog(&bytes);
    // The corpus has no entries 0-31: the reference's bare-bit lookup of
    // the last status flag finds nothing.
    assert!((0..32).all(|id| catalog.get(id).is_none()));
    for (attack, verbose, brief) in fixture_j() {
        assert_eq!(run(&catalog, &attack, true), verbose);
        assert_eq!(run(&catalog, &attack, false), brief);
    }
}

#[test]
fn spec007_line_order_wrong_target_and_attacker_lines() {
    let catalog = catalog(COMBAT.as_bytes());
    let ian = Named {
        name: b"Ian".to_vec(),
        ..Named::default()
    };
    // 1-4 in order: strength (no extra `.`), wrong target aimed at the
    // player (the `.` follows the message's own `!`), the struck target,
    // then the second attacker line for its self-damage.
    let attack = AttackLines {
        strength: true,
        attacker: Some(raider()),
        aimed: Some(you()),
        struck: Some(Struck::Critter(ian.clone())),
        wrong_target: true,
        location: location::UNCALLED,
        damage: 4,
        flags: effect::HIT | effect::KNOCKED_DOWN,
        attacker_damage: 2,
        attacker_flags: effect::LOSE_TURN,
        ..AttackLines::default()
    };
    assert_eq!(
        run(&catalog, &attack, true),
        [
            "You are not strong enough to use this weapon properly.",
            "Oops! Ian was hit instead of you!.",
            "Ian was hit for 4 hit points and knocked down.",
            "Raider was hit for 2 hit points and lost his next turn.",
        ]
    );
    // Aimed at someone else: B + 9 with the struck, then the aimed name.
    let attack = AttackLines {
        aimed: Some(raider()),
        attacker: Some(you()),
        ..attack
    };
    assert_eq!(
        run(&catalog, &attack, true)[1],
        "Oops! Ian was hit instead of Raider!."
    );
    // A struck wall: the attacker's "missed"; no struck-target line.
    let wall = AttackLines {
        attacker: Some(raider()),
        aimed: Some(you()),
        struck: Some(Struck::Object),
        wrong_target: true,
        ..AttackLines::default()
    };
    assert_eq!(run(&catalog, &wall, true), ["Raider missed."]);
    // An ordinary miss, with the attacker's status list; a dead attacker
    // gets no line.
    let miss = AttackLines {
        attacker: Some(you()),
        aimed: Some(raider()),
        ..AttackLines::default()
    };
    assert_eq!(run(&catalog, &miss, true), ["You missed."]);
    let dead = AttackLines {
        attacker: Some(Named {
            dead: true,
            ..you()
        }),
        ..miss.clone()
    };
    assert!(run(&catalog, &dead, true).is_empty());
    // A critical miss for 0 and 1 self-damage.
    let critical = |damage| AttackLines {
        critical_miss: true,
        attacker_damage: damage,
        attacker_flags: effect::CRITICAL,
        ..miss.clone()
    };
    assert_eq!(
        run(&catalog, &critical(0), true),
        ["You critically missed."]
    );
    assert_eq!(
        run(&catalog, &critical(1), true),
        ["You critically missed and took 1 hit point."]
    );
}

#[test]
fn spec007_struck_target_wordings_dead_targets_and_extras() {
    let catalog = catalog(COMBAT.as_bytes());
    // Damage 0 and 1 have no number; anything else, negative too, is %d.
    for (damage, expected) in [
        (0, "Raider was hit in the head for no damage."),
        (1, "Raider was hit in the head for 1 hit point."),
        (-3, "Raider was hit in the head for -3 hit points."),
    ] {
        assert_eq!(run(&catalog, &head_hit(damage, 0, None), false), [expected]);
    }
    // A torso critical without a critical-effect message keeps the status
    // list in Verbose too.
    let torso = AttackLines {
        location: location::TORSO,
        ..head_hit(0, 0, None)
    };
    let critical = AttackLines {
        critical: true,
        flags: effect::HIT | effect::CRITICAL,
        ..torso.clone()
    };
    assert_eq!(
        run(&catalog, &critical, true),
        ["Raider was critically hit for no damage."]
    );
    // A missing location name starts the line empty.
    let unnamed = AttackLines {
        struck: Some(Struck::Critter(Named {
            alias: None,
            ..raider()
        })),
        ..head_hit(5, effect::KNOCKED_DOWN, None)
    };
    assert_eq!(run(&catalog, &unnamed, true), [" and knocked down."]);
    // A target already dead before the attack gets no line.
    let dead = AttackLines {
        struck: Some(Struck::Critter(Named {
            dead: true,
            ..raider()
        })),
        ..head_hit(5, 0, None)
    };
    assert!(run(&catalog, &dead, true).is_empty());
    // Extra victims: the torso wording and status list, unless dead; a
    // female NPC uses base 700.
    let female = Named {
        name: b"Katja".to_vec(),
        female: true,
        ..Named::default()
    };
    let spray = AttackLines {
        extras: vec![
            (female, 6, effect::HIT | effect::KNOCKED_DOWN),
            (
                Named {
                    dead: true,
                    ..raider()
                },
                9,
                effect::HIT,
            ),
            (raider(), 0, effect::HIT),
        ],
        ..head_hit(5, 0, None)
    };
    assert_eq!(
        run(&catalog, &spray, true),
        [
            "Raider was hit in the head for 5 hit points.",
            "Katja was hit for 6 hit points and knocked down.",
            "Raider was hit for no damage.",
        ]
    );
}

#[test]
fn spec007_status_lists_and_verbose_killed_lines() {
    let catalog = catalog(COMBAT.as_bytes());
    let text = |id: i32| catalog.get(id).map(|m| m.text.clone()).unwrap_or_default();
    let list = |critter: &Named, flags: u32| {
        String::from_utf8(status_list(critter, flags, &text)).unwrap()
    };
    // Nothing for no flags, or only hit and critical.
    assert_eq!(list(&raider(), 0), "");
    assert_eq!(list(&raider(), effect::HIT | effect::CRITICAL), "");
    // Dead alone, whatever else is set.
    assert_eq!(
        list(&raider(), effect::DEAD | effect::KNOCKED_DOWN),
        " and was killed"
    );
    // Every bit from bit 0; the last after " and " (the safe rule).
    assert_eq!(
        list(
            &raider(),
            effect::KNOCKED_DOWN | effect::CRIPPLED_LEFT_LEG | 0x800
        ),
        ", knocked down, crippled his left leg and had his armor bypassed"
    );
    // The player's base is 200 whatever the gender.
    let female_player = Named {
        female: true,
        ..you()
    };
    assert_eq!(
        list(&female_player, effect::KNOCKED_OUT),
        " and knocked out"
    );
    // Verbose "killed" line: 207 or 257 for the player by gender.
    let killed = |struck: Named| AttackLines {
        attacker: Some(raider()),
        aimed: Some(struck.clone()),
        struck: Some(Struck::Critter(struck)),
        location: location::TORSO,
        damage: 30,
        flags: effect::HIT | effect::CRITICAL | effect::DEAD,
        critical: true,
        critical_message: Some(5007),
        ..AttackLines::default()
    };
    assert_eq!(
        run(&catalog, &killed(female_player), true),
        [
            "You were critically hit for 30 hit points, death is instantaneous.",
            "You were killed.",
        ]
    );
    assert_eq!(run(&catalog, &killed(you()), true)[1], "You were killed.");
    // Brief: the status list instead.
    assert_eq!(
        run(&catalog, &killed(raider()), false),
        ["Raider was critically hit for 30 hit points and was killed."]
    );
}
