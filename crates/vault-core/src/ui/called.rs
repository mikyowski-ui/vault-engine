//! SPEC-014 §3.38 the called-shot window: the attack preflight's
//! `combat.msg` refusals, the window layout (`called.frm`, the `na` critter
//! picture, Cancel), the eight body parts with their selection codes and
//! hit locations, the chance digits, the label messages chosen by art
//! alias, the invisible part buttons and the input codes. The presentation
//! layer draws it and drives its input; SPEC-007 §3.5 supplies the chances
//! and §3.6 the attack. Coordinates are window-relative.
use super::Rect;
use crate::{
    Refusal, WorldError,
    combat::Attack,
    critical::location::{EYES, GROIN, HEAD, LEFT_ARM, LEFT_LEG, RIGHT_ARM, RIGHT_LEG, TORSO},
};

/// The window's screen origin and size: horizontally centered, above the
/// interface bar.
pub const ORIGIN: [i32; 2] = [108, 20];
pub const SIZE: [i32; 2] = [424, 309];
/// The opaque background (list index 118). Its art holds the frame, the
/// chance boxes, the leader lines and the word CANCEL.
pub const BACKGROUND: &str = "called.frm";
/// The critter picture: animation code 64 (`na`), weapon code 0, selector
/// 0, frame 0 copied opaquely as 170 by 225 at (128, 31).
pub const PICTURE_ANIMATION: u8 = 64;
pub const PICTURE: Rect = Rect::new(128, 31, 170, 225);
/// The Cancel red button (`lilredup.frm` / `lilreddn.frm`), drawn
/// transparently; its release returns Esc.
pub const CANCEL: [i32; 2] = [170, 268];
/// Played when a part is chosen, as the window closes.
pub const CHOSEN_SOUND: &str = "icsxxxx1";
/// Row tops, rows 0-3.
pub const ROW_TOPS: [i32; 4] = [36, 102, 166, 230];
/// The chance digits' x, left and right parts.
pub const CHANCE_X: [i32; 2] = [33, 373];
/// A left label starts here; a right label of width w is drawn at
/// `351 - w`, so it ends at x = 350.
pub const LEFT_LABEL_X: i32 = 74;
pub const RIGHT_LABEL_END: i32 = 351;
/// The invisible part button: 88 by 10, 4 rows below the label top.
pub const BUTTON_SIZE: [i32; 2] = [88, 10];
pub const BUTTON_DROP: i32 = 4;
/// A missing right label counts as 10 wide for its button.
pub const MISSING_LABEL_WIDTH: i32 = 10;
/// `numbers.frm` white band: digit d is the 9-by-17 cell at x = 9d; the
/// minus glyph is 6 wide at x = 108.
pub const DIGIT_WIDTH: i32 = 9;
pub const DIGIT_HEIGHT: i32 = 17;
pub const MINUS_X: i32 = 108;
pub const MINUS_WIDTH: i32 = 6;
/// Label messages: `combat.msg` 1000 + 10 × alias + location.
pub const LABEL_BASE: i32 = 1000;

/// The hit location of each selection code 0-7 (the button index): the
/// left parts head, eyes, right arm, right leg, then the right parts
/// torso, groin, left arm, left leg.
pub const LOCATIONS: [u8; 8] = [
    HEAD, EYES, RIGHT_ARM, RIGHT_LEG, TORSO, GROIN, LEFT_ARM, LEFT_LEG,
];

/// One body part of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part {
    /// The selection code, 0-7.
    pub code: u8,
    /// The hit location, 0-7.
    pub location: u8,
    pub row: usize,
    pub right: bool,
}
impl Part {
    pub fn of(code: u8) -> Option<Self> {
        let location = *LOCATIONS.get(usize::from(code))?;
        Some(Self {
            code,
            location,
            row: usize::from(code % 4),
            right: code >= 4,
        })
    }
    pub fn top(&self) -> i32 {
        ROW_TOPS[self.row]
    }
    /// The tens cell of the chance; the ones cell is 9 pixels right.
    pub fn chance_origin(&self) -> [i32; 2] {
        [CHANCE_X[usize::from(self.right)], self.top()]
    }
    /// Where a label `width` pixels wide is drawn.
    pub fn label_origin(&self, width: i32) -> [i32; 2] {
        if self.right {
            [RIGHT_LABEL_END - width, self.top()]
        } else {
            [LEFT_LABEL_X, self.top()]
        }
    }
    /// The invisible button, from the label's width (`None`: no label). It
    /// does not follow the text: on the left it covers the chance digits
    /// and label columns 74-120 only; on the right it runs 88 pixels from
    /// the label's left edge.
    pub fn button(&self, label_width: Option<i32>) -> Rect {
        let x = if self.right {
            RIGHT_LABEL_END - label_width.unwrap_or(MISSING_LABEL_WIDTH)
        } else {
            CHANCE_X[0]
        };
        Rect::new(x, self.top() + BUTTON_DROP, BUTTON_SIZE[0], BUTTON_SIZE[1])
    }
}

/// The parts in drawing order: each row's left part, then its right part.
pub fn parts() -> impl Iterator<Item = Part> {
    (0..4u8).flat_map(|row| [row, row + 4]).filter_map(Part::of)
}

/// The two `numbers.frm` cells of a chance, as (source x, width, x offset
/// from the chance origin): tens then ones, with a leading zero below 10.
/// A negative chance draws the 6-wide minus glyph in both cells, so every
/// negative chance looks the same. The SPEC-007 §3.5 cap keeps a chance at
/// 95 or below; a larger value shows 99.
pub fn chance_cells(chance: i32) -> [(i32, i32, i32); 2] {
    if chance < 0 {
        return [
            (MINUS_X, MINUS_WIDTH, 0),
            (MINUS_X, MINUS_WIDTH, DIGIT_WIDTH),
        ];
    }
    let chance = chance.min(99);
    [
        (DIGIT_WIDTH * (chance / 10), DIGIT_WIDTH, 0),
        (DIGIT_WIDTH * (chance % 10), DIGIT_WIDTH, DIGIT_WIDTH),
    ]
}

/// The `combat.msg` label of `location` for the target art's critter-list
/// alias.
pub fn label_message(alias: i32, location: u8) -> Option<i32> {
    alias
        .checked_mul(10)?
        .checked_add(LABEL_BASE)?
        .checked_add(i32::from(location))
}

/// The SPEC-007 §3.5 range-aware hit chance at each location 0-7 for an
/// attack profile (its own location is ignored): the location adjustment
/// is full for ranged and thrown attacks and half for melee and unarmed.
/// No draw is made.
pub fn chances(attack: &Attack) -> Result<[i32; 8], WorldError> {
    let mut chances = [0; 8];
    for (location, chance) in chances.iter_mut().enumerate() {
        let mut aimed = attack.clone();
        aimed.location = location as u8;
        *chance = aimed.hit_chance(true)?;
    }
    Ok(chances)
}

/// The `combat.msg` line a failing preflight test prints (§3.38 step 2):
/// none for a dead target, 105 both arms crippled, 106 a two-handed weapon
/// with a crippled arm, 100 "You need %d action points." (with the cost),
/// 102 out of range, 101 out of ammo (with the weapon's out-of-ammo
/// sound), 104 aim blocked.
pub fn refusal_message(refusal: Refusal) -> Option<i32> {
    match refusal {
        Refusal::DeadDefender | Refusal::Sequence => None,
        Refusal::CrippledArms => Some(105),
        Refusal::CrippledArmTwoHanded => Some(106),
        Refusal::ActionPoints => Some(100),
        Refusal::Range => Some(102),
        Refusal::NoAmmunition => Some(101),
        Refusal::Obstructed => Some(104),
    }
}

/// The input code a key makes in the window: Ctrl+A through Ctrl+H give
/// 1-8 (by letter) and Backspace 8. Digit keys and every other key make
/// none.
pub fn key_code(letter: Option<char>, control: bool, backspace: bool) -> Option<u8> {
    if backspace {
        return Some(8);
    }
    let letter = letter?.to_ascii_lowercase();
    (control && ('a'..='h').contains(&letter)).then(|| letter as u8 - b'a' + 1)
}

/// What an input code chooses: codes 0-7 a part by its selection code.
/// Code 8 reads past the end of the right-hand table in the reference
/// (`[May-improve]`): it is ignored and the window stays open.
pub fn choice(code: u8) -> Option<Part> {
    Part::of(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        combat::{PlayerModifiers, WeaponProfile, attack_mode},
        critical::{hit_mode, location},
    };

    /// SPEC-007 Fixture C's rifle shot: skill 80, range -12, cover -10 and
    /// AC -12 sum to 46 before the location adjustment.
    fn fixture_c() -> Attack {
        Attack {
            attacker_player: true,
            attacker_pid: 0x0100_0000,
            attacker_team: 0,
            player_team: 0,
            skill: 80,
            perception: 6,
            strength: 5,
            luck: 5,
            critical_chance: 5,
            better_criticals: 20,
            melee_damage: 1,
            blind: false,
            player: PlayerModifiers::default(),
            jinxed: false,
            weapon: Some(WeaponProfile {
                pid: 9,
                mode: attack_mode::SINGLE,
                damage: [8, 20],
                damage_type: 0,
                range: 20,
                min_strength: 4,
                ap_cost: 5,
                failure_table: 0,
                burst: 0,
                two_handed: false,
                big_gun: false,
                perk: None,
                rounds: 6,
                capacity: 6,
            }),
            hit_mode: hit_mode::RIGHT_PRIMARY,
            location: location::UNCALLED,
            aimed: true,
            distance: 15,
            cover: 1,
            defender_player: false,
            defender_pid: 0x0100_0002,
            kill_category: 0,
            armor_class: 12,
            thresholds: [0; 7],
            resistances: [0; 7],
            attacker_thresholds: [0; 7],
            attacker_resistances: [0; 7],
            defender_special: [5; 7],
            defender_multihex: false,
            defender_prone: false,
            defender_light: 60000,
            defender_hp: 60,
            ticks: 864_000,
            combat_difficulty: 1,
            accuracy_bonus: 0,
            tables: None,
        }
    }

    /// The chance of each selection code.
    fn by_code(chances: [i32; 8]) -> [i32; 8] {
        LOCATIONS.map(|location| chances[usize::from(location)])
    }

    #[test]
    fn spec014_fixture_h_chances_by_part() {
        // Head 6, eyes -14, right arm 16, right leg 26; torso 46, groin 16,
        // left arm 16, left leg 26.
        let attack = fixture_c();
        assert_eq!(
            by_code(chances(&attack).unwrap()),
            [6, -14, 16, 26, 46, 16, 16, 26]
        );
        // The aimed right leg is Fixture C's 26 at 5 + 1 = 6 AP.
        let mut leg = attack.clone();
        leg.location = location::RIGHT_LEG;
        assert_eq!((leg.hit_chance(true).unwrap(), leg.ap_cost()), (26, 6));
        // Torso is location 3, still aimed: the same 0 adjustment, 6 AP.
        let mut torso = attack.clone();
        torso.location = Part::of(4).unwrap().location;
        assert_eq!(torso.location, location::TORSO);
        assert_eq!((torso.hit_chance(true).unwrap(), torso.ap_cost()), (46, 6));
        // A melee weapon whose other terms also sum to 46 takes half
        // adjustments.
        let mut melee = attack.clone();
        melee.weapon.as_mut().unwrap().mode = attack_mode::SWING;
        melee.armor_class = 34;
        assert_eq!(chances(&melee).unwrap(), [26, 31, 31, 46, 36, 36, 16, 31]);
        // Other terms summing to 120, ranged: the cap of 95.
        let mut sure = attack;
        sure.skill = 154;
        assert_eq!(chances(&sure).unwrap(), [80, 90, 90, 95, 95, 95, 60, 90]);
    }

    #[test]
    fn spec014_fixture_h_chance_cells() {
        let cells = |code: u8, chance: i32| {
            let part = Part::of(code).unwrap();
            let [x, y] = part.chance_origin();
            chance_cells(chance).map(|(source, width, dx)| (source, width, [x + dx, y]))
        };
        assert_eq!(
            cells(0, 6),
            [(0, 9, [33, 36]), (54, 9, [42, 36])],
            "head 06"
        );
        assert_eq!(cells(4, 46), [(36, 9, [373, 36]), (54, 9, [382, 36])]);
        assert_eq!(
            cells(1, -14),
            [(108, 6, [33, 102]), (108, 6, [42, 102])],
            "eyes: two minus glyphs"
        );
        assert_eq!(cells(5, 16), [(9, 9, [373, 102]), (54, 9, [382, 102])]);
        assert_eq!(cells(2, 16)[0].2, [33, 166]);
        assert_eq!(cells(6, 16)[1].2, [382, 166]);
        assert_eq!(cells(3, 26)[1].2, [42, 230]);
        assert_eq!(cells(7, 26)[0].2, [373, 230]);
        assert_eq!(chance_cells(-1), chance_cells(-90));
    }

    #[test]
    fn spec014_fixture_h_labels_and_buttons() {
        // Target base index 13 (`hmmaxx`) has alias 11.
        assert_eq!(label_message(11, location::HEAD), Some(1110));
        assert_eq!(label_message(11, location::TORSO), Some(1113));
        assert_eq!(label_message(11, location::LEFT_LEG), Some(1115));
        assert_eq!(label_message(15, location::LEFT_LEG), Some(1155));
        assert_eq!(label_message(0, location::GROIN), Some(1007));
        let head = Part::of(0).unwrap();
        assert_eq!(head.label_origin(41), [74, 36]);
        assert_eq!(head.button(Some(41)), Rect::new(33, 40, 88, 10));
        let torso = Part::of(4).unwrap();
        assert_eq!(torso.label_origin(60), [291, 36]);
        assert_eq!(torso.button(Some(60)), Rect::new(291, 40, 88, 10));
        let left_leg = Part::of(7).unwrap();
        assert_eq!(left_leg.label_origin(71), [280, 230]);
        assert_eq!(left_leg.button(Some(71)), Rect::new(280, 234, 88, 10));
        assert_eq!(left_leg.label_origin(97), [254, 230], "the left foreleg");
        assert_eq!(left_leg.button(Some(97)), Rect::new(254, 234, 88, 10));
        assert_eq!(left_leg.button(None), Rect::new(341, 234, 88, 10));
        // A release at (40, 236) is on the right-leg button; at (125, 236)
        // it is outside it, though "the right leg" is drawn through 151.
        let right_leg = Part::of(3).unwrap();
        assert_eq!(
            (right_leg.location, right_leg.row, right_leg.right),
            (location::RIGHT_LEG, 3, false)
        );
        let hit = |point| {
            parts()
                .find(|part| part.button(Some(78)).contains(point))
                .map(|part| part.code)
        };
        assert_eq!(hit([40, 236]), Some(3));
        assert_eq!(hit([125, 236]), None);
        // Drawing order: each row's left part, then its right part.
        let order: Vec<u8> = parts().map(|part| part.code).collect();
        assert_eq!(order, [0, 4, 1, 5, 2, 6, 3, 7]);
    }

    #[test]
    fn spec014_fixture_h_input_codes() {
        // Ctrl+A-G choose eyes, right arm, right leg, torso, groin, left
        // arm and left leg; Ctrl+D is the torso, location 3.
        let chosen: Vec<u8> = ('a'..='g')
            .map(|letter| {
                let code = key_code(Some(letter), true, false).unwrap();
                choice(code).unwrap().location
            })
            .collect();
        assert_eq!(
            chosen,
            [EYES, RIGHT_ARM, RIGHT_LEG, TORSO, GROIN, LEFT_ARM, LEFT_LEG]
        );
        // Digit keys and plain letters do nothing.
        assert_eq!(key_code(Some('4'), false, false), None);
        assert_eq!(key_code(Some('4'), true, false), None);
        assert_eq!(key_code(Some('d'), false, false), None);
        // Code 8 (Backspace, Ctrl+H) is ignored (`[May-improve]`).
        assert_eq!(key_code(None, false, true), Some(8));
        assert_eq!(key_code(Some('h'), true, false), Some(8));
        assert_eq!(choice(8), None);
        assert_eq!(choice(0).unwrap().location, HEAD);
    }

    #[test]
    fn spec014_preflight_refusal_messages() {
        assert_eq!(refusal_message(Refusal::DeadDefender), None);
        assert_eq!(refusal_message(Refusal::CrippledArms), Some(105));
        assert_eq!(refusal_message(Refusal::CrippledArmTwoHanded), Some(106));
        assert_eq!(refusal_message(Refusal::ActionPoints), Some(100));
        assert_eq!(refusal_message(Refusal::Range), Some(102));
        assert_eq!(refusal_message(Refusal::NoAmmunition), Some(101));
        assert_eq!(refusal_message(Refusal::Obstructed), Some(104));
    }
}
