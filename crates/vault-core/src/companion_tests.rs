//! The enhanced companion state's sidecar text and distance modes.
use super::*;

#[test]
fn the_sidecar_round_trips_and_names_bad_lines() {
    let mut distances = BTreeMap::new();
    distances.insert(18000, Distance::Close);
    distances.insert(18004, Distance::Charge);
    distances.insert(18009, Distance::Default);
    let text = write_sidecar(&distances);
    distances.remove(&18009);
    assert_eq!(parse_sidecar(&text).unwrap(), distances);
    assert!(parse_sidecar("").unwrap().is_empty());
    for (text, needle) in [
        ("18000 = \"close\"", "before `[companion_distance]`"),
        ("[companion_distance]\nx = \"close\"", "not an object id"),
        ("[companion_distance]\n18000 = \"near\"", "unknown distance"),
        ("[party]", "unknown table"),
        ("[companion_distance]\n18000", "expected"),
    ] {
        let error = parse_sidecar(text).unwrap_err();
        assert!(error.contains(needle), "{text}: {error}");
    }
}

#[test]
fn the_distance_modes_cycle_and_name_themselves() {
    let mut mode = Distance::default();
    let mut seen = Vec::new();
    for _ in 0..4 {
        seen.push(mode.name());
        assert_eq!(Distance::parse(mode.name()), Some(mode));
        mode = mode.next();
    }
    assert_eq!(seen, ["default", "close", "hold", "charge"]);
    assert_eq!(mode, Distance::Default);
}
