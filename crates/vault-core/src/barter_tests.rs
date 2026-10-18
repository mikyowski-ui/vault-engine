//! SPEC-011 fixture F and the barter session: the binary64 price formula,
//! the reaction and script modifiers, Master Trader, acceptance, and the
//! merchant equipment hidden for the session.
use crate::{
    ObjectRef, Perk, WorldHost,
    barter::{MINIMUM_FACTOR, Trade, price, reaction_modifier},
    fixture_items::{self as support, ARMOR_A, ItemAssets, MONEY, WEAPON_W, messages},
};
use vault_formats::pro::BasicFlags;

#[test]
fn spec011_fixture_f_prices() {
    // V_T = 230 with 30 caps (g = 200); B_p = 50, B_m = 40.
    let f = |modifier, master| price(230, 30, modifier, master, 50, 40);
    assert_eq!(f(0, false), 410, "F1");
    assert_eq!(f(10 - 15, true), 296, "F2");
    assert_eq!(f(10, true), 353, "F2 first frame (reference)");
    assert_eq!(f(25, false), 506, "F3");
    assert_eq!(f(-200, false), 33, "F4 clamps the factor");
    assert_eq!(price(0, 0, 25, true, 50, 40), 0, "F5 empty table");
    assert_eq!(price(50, 0, 0, false, 100, 100), 100, "twice the value");
    assert_eq!(format!("{MINIMUM_FACTOR:.10}"), "0.0099999998");
    assert_eq!(
        [10, 25, 26, 50, 75, 76, 80].map(reaction_modifier),
        [25, 25, 0, 0, 0, -15, -15]
    );
}

/// A merchant with Barter 40 (base 30 + 10), cached reaction `reaction`,
/// worn armor, and a right-hand weapon; the player has Barter 50.
fn market(reaction: i32) -> (WorldHost<ItemAssets>, ObjectRef, [ObjectRef; 2]) {
    let mut host = support::host();
    host.world.player_sheet_mut().unwrap().invested[15] = 20;
    let prototype = support::critter(0x0100_0301, &[], 0, &[(15, 10)]);
    let merchant = host.world.spawn(prototype, 20300, 0).unwrap();
    let sid = host.world.add_script(merchant, 0, 4, 4).unwrap();
    let base = host.world.ensure_locals(sid).unwrap();
    host.world.lvars[base] = reaction;
    host.world.lvars[base + 2] = 1;
    let armour = support::carry(&mut host, merchant, ARMOR_A, 1);
    host.world
        .equip(merchant, armour, BasicFlags::WORN.0)
        .unwrap();
    let weapon = support::carry(&mut host, merchant, WEAPON_W, 1);
    host.world
        .equip(merchant, weapon, BasicFlags::RIGHT_HAND.0)
        .unwrap();
    host.assets
        .prototypes
        .insert(0x700, support::misc(0x700, 1, 1, 200));
    host.assets
        .prototypes
        .insert(0x701, support::misc(0x701, 1, 1, 410));
    host.assets
        .prototypes
        .insert(0x702, support::misc(0x702, 1, 1, 409));
    (host, merchant, [armour, weapon])
}
fn holds(host: &WorldHost<ItemAssets>, owner: ObjectRef, item: ObjectRef) -> bool {
    host.world
        .get(owner)
        .unwrap()
        .inventory
        .iter()
        .any(|(entry, _)| *entry == item)
}

#[test]
fn spec011_barter_session_price_acceptance_and_returns() {
    let (mut host, merchant, [armour, weapon]) = market(50);
    let player = host.world.player;
    let goods = support::carry(&mut host, merchant, 0x700, 1);
    let caps = support::carry(&mut host, merchant, MONEY, 30);
    let session = host.begin_barter(merchant).unwrap();
    assert_eq!(session.hidden().collect::<Vec<_>>(), [armour, weapon]);
    assert!(!holds(&host, merchant, armour) && !holds(&host, merchant, weapon));
    assert_eq!(host.attempt_trade(&session).unwrap(), Trade::Nothing);
    host.barter_move(merchant, session.table, goods, 1).unwrap();
    host.barter_move(merchant, session.table, caps, 30).unwrap();
    assert_eq!(host.barter_modifier(&session), 0, "neutral reaction");
    assert_eq!(host.barter_price(&session), 410, "F1");
    // An empty offer, an offer worth 409, and an armed offer are refused.
    host.world.effects.clear();
    assert_eq!(host.attempt_trade(&session).unwrap(), Trade::Refused);
    let cheap = support::carry(&mut host, player, 0x702, 1);
    host.barter_move(player, session.offer, cheap, 1).unwrap();
    assert_eq!(host.attempt_trade(&session).unwrap(), Trade::Refused);
    assert_eq!(messages(&host), ["no deal", "no deal"]);
    host.barter_move(session.offer, player, cheap, 1).unwrap();
    let fair = support::carry(&mut host, player, 0x701, 1);
    host.barter_move(player, session.offer, fair, 1).unwrap();
    host.world.get_mut(fair).unwrap().raw.flags.0 |= BasicFlags::USED.0;
    assert_eq!(
        host.attempt_trade(&session).unwrap(),
        Trade::Refused,
        "armed"
    );
    host.world.get_mut(fair).unwrap().raw.flags.0 &= !BasicFlags::USED.0;
    host.world.effects.clear();
    assert_eq!(host.attempt_trade(&session).unwrap(), Trade::Accepted);
    assert_eq!(messages(&host), ["deal"]);
    assert!(holds(&host, player, goods) && holds(&host, merchant, fair));
    assert_eq!(host.world.caps(player), 30, "caps cross at face value");
    // Leaving returns the tables and restores the merchant's equipment.
    let leftover = support::carry(&mut host, player, 0x702, 1);
    host.barter_move(player, session.offer, leftover, 1)
        .unwrap();
    let (table, offer) = (session.table, session.offer);
    host.end_barter(session).unwrap();
    assert!(holds(&host, player, leftover));
    assert!(host.world.get(table).is_none() && host.world.get(offer).is_none());
    let flags = |item| host.world.get(item).unwrap().raw.flags;
    assert!(flags(armour).contains(BasicFlags::WORN));
    assert!(flags(weapon).contains(BasicFlags::RIGHT_HAND));
    assert!(holds(&host, merchant, armour) && holds(&host, merchant, weapon));
}

#[test]
fn spec011_barter_modifiers_master_trader_and_fallback_weapon() {
    // F2: script modifier 10, good reaction (-15), and Master Trader.
    let (mut host, merchant, [_, weapon]) = market(80);
    host.world.dialogue.barter_modifier = 10;
    host.world
        .player_sheet_mut()
        .unwrap()
        .perks
        .insert(Perk::MasterTrader, 1);
    let goods = support::carry(&mut host, merchant, 0x700, 1);
    let caps = support::carry(&mut host, merchant, MONEY, 30);
    // Without a right-hand item the first weapon is hidden, and it comes
    // back unequipped.
    host.world.get_mut(weapon).unwrap().raw.flags.0 &= !BasicFlags::RIGHT_HAND.0;
    let session = host.begin_barter(merchant).unwrap();
    assert!(session.hidden().any(|item| item == weapon));
    host.barter_move(merchant, session.table, goods, 1).unwrap();
    host.barter_move(merchant, session.table, caps, 30).unwrap();
    assert_eq!(host.barter_modifier(&session), -5);
    assert_eq!(host.barter_price(&session), 296, "F2");
    host.end_barter(session).unwrap();
    let flags = host.world.get(weapon).unwrap().raw.flags;
    assert!(holds(&host, merchant, weapon) && flags.0 & 0x0700_0000 == 0);
    // A new dialogue resets the script modifier.
    host.world.dialogue.start(merchant, 1, -1, None).unwrap();
    assert_eq!(host.world.dialogue.barter_modifier, 0);
}
