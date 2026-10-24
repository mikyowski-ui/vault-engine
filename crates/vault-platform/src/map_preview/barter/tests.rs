//! H13 and M-I5: the barter window's open containers and offer total
//! (SPEC-011 §3.19, SPEC-013 §3.18) over the synthetic corpus.
use crate::map_preview::{MapPreview, Screen, fixture, state_fixture};
use std::sync::Arc;
use vault_core::{ObjectRef, skills::ItemEntry};
use vault_formats::{LocalDirProvider, Vfs, pro::Category};

fn preview(root: &std::path::Path) -> MapPreview {
    let vfs: Arc<dyn Vfs> = Arc::new(LocalDirProvider::mount(root).unwrap());
    MapPreview::load(
        vfs,
        "maps/synthetic.map",
        "english",
        Some("premade/synthetic.gcd"),
        1,
    )
    .unwrap()
}
/// An item of `subtype` (1 container, 5 miscellaneous) worth `cost`,
/// carried by `owner`.
fn item(
    preview: &mut MapPreview,
    pid: i32,
    subtype: usize,
    cost: i32,
    owner: ObjectRef,
) -> ObjectRef {
    let mut bytes = fixture::prototype(0, subtype);
    fixture::word(&mut bytes, 0, pid);
    fixture::word(&mut bytes, 0x30, cost);
    let prototype = Arc::new(vault_formats::Prototype::parse(&bytes).unwrap());
    let item = preview.host.world.spawn(prototype, 0, 0).unwrap();
    preview.host.world.add_item(owner, item, 1, true).unwrap();
    item
}
fn screen(preview: &MapPreview) -> &super::BarterScreen {
    match &preview.screen {
        Screen::Barter(screen) => screen,
        _ => panic!("the barter window is open"),
    }
}
fn screen_mut(preview: &mut MapPreview) -> &mut super::BarterScreen {
    match &mut preview.screen {
        Screen::Barter(screen) => screen,
        _ => panic!("the barter window is open"),
    }
}

#[test]
fn h13_an_accepted_trade_closes_table_containers_and_m_i5_totals_the_whole_table() {
    let dir = tempfile::tempdir().unwrap();
    state_fixture::write_corpus(dir.path());
    let mut preview = preview(dir.path());
    let player = preview.host.world.player;
    let merchant = preview
        .host
        .world
        .iter()
        .find(|(reference, object)| object.category() == Category::Critter && *reference != player)
        .unwrap()
        .0;
    // A bag worth 10 holding a widget worth 25.
    let bag = item(&mut preview, 0x41, 1, 10, player);
    let widget = item(&mut preview, 0x40, 5, 25, bag);
    let dialogue = &mut preview.host.world.dialogue;
    dialogue.open = true;
    dialogue.target = merchant;
    dialogue.pending_barter = true;
    preview.check_barter();
    let offer = screen(&preview).session.offer;
    // The bag goes on the player table and is opened there.
    preview.host.barter_move(player, offer, bag, 1).unwrap();
    preview.barter_entry(1, bag, ItemEntry::Use);
    assert_eq!(screen(&preview).views[1], [bag]);
    assert_eq!(preview.barter_owners().unwrap()[1], bag);
    // M-I5: the total is the whole table's, not the open bag's.
    let total = preview.host.world.inventory_value(offer);
    assert_ne!(total, preview.host.world.inventory_value(bag));
    assert_eq!(preview.barter_offer_value(), total);
    preview.image().unwrap();
    // An empty merchant table costs nothing: the gift is accepted, the bag
    // changes sides, and its view on the table closes.
    screen_mut(&mut preview).offsets[1] = 2;
    preview.attempt_barter();
    assert_eq!(preview.host.world.get(bag).unwrap().owner, Some(merchant));
    assert_eq!(preview.host.world.get(widget).unwrap().owner, Some(bag));
    assert!(screen(&preview).views[1].is_empty());
    assert!(screen(&preview).views[2].is_empty());
    assert_eq!(screen(&preview).offsets[1..3], [0, 0]);
    assert_eq!(preview.barter_owners().unwrap()[1], offer);
    preview.image().unwrap();
}

#[test]
fn h13_each_frame_cuts_a_broken_container_chain_and_clamps_the_offsets() {
    let dir = tempfile::tempdir().unwrap();
    state_fixture::write_corpus(dir.path());
    let mut preview = preview(dir.path());
    let player = preview.host.world.player;
    let merchant = preview
        .host
        .world
        .iter()
        .find(|(reference, object)| object.category() == Category::Critter && *reference != player)
        .unwrap()
        .0;
    let bag = item(&mut preview, 0x41, 1, 10, player);
    let inner = item(&mut preview, 0x42, 1, 5, bag);
    let dialogue = &mut preview.host.world.dialogue;
    dialogue.open = true;
    dialogue.target = merchant;
    dialogue.pending_barter = true;
    preview.check_barter();
    preview.barter_entry(0, bag, ItemEntry::Use);
    preview.barter_entry(0, inner, ItemEntry::Use);
    assert_eq!(screen(&preview).views[0], [bag, inner]);
    // A chain that still holds is kept.
    preview.tick(1000).unwrap();
    assert_eq!(screen(&preview).views[0], [bag, inner]);
    // The inner bag leaves its holder (to the merchant): the chain is cut
    // there, and the column shows the outer bag.
    preview.host.world.remove_item(bag, inner, 1).unwrap();
    preview
        .host
        .world
        .add_item(merchant, inner, 1, true)
        .unwrap();
    // A container the merchant does not hold is not a valid view either.
    screen_mut(&mut preview).views[3] = vec![bag];
    screen_mut(&mut preview).offsets[0] = 7;
    preview.tick(1041).unwrap();
    assert_eq!(screen(&preview).views[0], [bag]);
    assert!(screen(&preview).views[3].is_empty());
    // The outer bag is now empty: the offset returns to the top.
    assert_eq!(screen(&preview).offsets[0], 0);
    preview.image().unwrap();
}
