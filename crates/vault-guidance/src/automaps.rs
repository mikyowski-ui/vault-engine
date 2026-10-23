//! SPEC-014 §3.41.4 the Pip-Boy Automaps page's grouping: the fixed
//! related-map table, the area list built from the captured maps of the
//! automap database (SPEC-008 §2.8), and the elevations listed for a chosen
//! area. Message texts and sorting by name are the caller's.
use vault_formats::{AutomapDb, automap::MAPS};

/// The related-map row of `map`: up to five map numbers in position order;
/// maps not listed have five empty positions. Only the Automaps page uses
/// the relation.
pub fn related(map: usize) -> &'static [usize] {
    match map {
        3 => &[9, 4, 5],
        4 => &[3, 9, 5],
        5 => &[3, 4, 9],
        9 => &[3, 4, 5],
        6 => &[35],
        35 => &[6],
        7 => &[8],
        8 => &[7],
        10 => &[11, 12],
        11 => &[10, 12],
        12 => &[10, 11],
        13 => &[14, 15, 55],
        14 => &[13, 15, 55],
        15 => &[13, 14, 55],
        55 => &[13, 14, 15],
        16 => &[25, 26],
        25 => &[16, 26],
        26 => &[16, 25],
        17 => &[18, 47, 33, 34],
        18 => &[17, 47, 33, 34],
        33 => &[34, 47, 17, 18],
        34 => &[33, 47, 17, 18],
        47 => &[17, 18, 33, 34],
        27 => &[42, 43],
        42 => &[27, 43],
        43 => &[27, 42],
        28 => &[29, 44, 45, 46],
        29 => &[28, 44, 45, 46],
        44 => &[28, 29, 45, 46],
        45 => &[28, 29, 44, 46],
        46 => &[28, 29, 44, 45],
        30 => &[31, 32, 48],
        31 => &[30, 32, 48],
        32 => &[30, 31, 48],
        48 => &[30, 31, 32],
        // 36 through 41: the other five, ascending.
        36 => &[37, 38, 39, 40, 41],
        37 => &[36, 38, 39, 40, 41],
        38 => &[36, 37, 39, 40, 41],
        39 => &[36, 37, 38, 40, 41],
        40 => &[36, 37, 38, 39, 41],
        41 => &[36, 37, 38, 39, 40],
        _ => &[],
    }
}

/// Whether map `map`, elevation `elevation` has a captured raster: a
/// positive database offset.
pub fn captured(db: &AutomapDb, map: usize, elevation: usize) -> bool {
    db.offset(map, elevation).is_some_and(|offset| offset > 0)
}

/// The area groups, each under its lowest-numbered captured map: maps 0 to
/// 65 are scanned, and a map with a positive offset on at least one
/// elevation joins unless an already-listed map is in its related row.
/// The result is in scan order; the page sorts it by short name.
pub fn area_groups(captured: impl Fn(usize, usize) -> bool) -> Vec<usize> {
    let mut groups: Vec<usize> = Vec::new();
    for map in 0..MAPS {
        if (0..3).any(|elevation| captured(map, elevation))
            && !related(map).iter().any(|other| groups.contains(other))
        {
            groups.push(map);
        }
    }
    groups
}

/// The elevation entries of area `area` as `(map, elevation)`: first each
/// captured elevation of `area` itself, from 0 to 2, then, for each map M
/// of its related row in position order, M's captured elevations. The
/// SPEC-014 §3.41.4 safe rule: R1 lists map number *i* for position i
/// instead of the map stored there.
pub fn elevation_entries(
    area: usize,
    captured: impl Fn(usize, usize) -> bool,
) -> Vec<(usize, usize)> {
    std::iter::once(area)
        .chain(related(area).iter().copied().filter(|map| *map != area))
        .flat_map(|map| (0..3).map(move |elevation| (map, elevation)))
        .filter(|(map, elevation)| captured(*map, *elevation))
        .collect()
}
