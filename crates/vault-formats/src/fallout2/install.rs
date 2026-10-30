//! SPEC-018 §2.2: the required members of a Fallout 2 install. Finding and
//! opening its archives in a folder is [`super::folder`]'s.
use crate::Dat2Provider;

/// §2.2 the members the master database must hold, as mounted.
pub const REQUIRED_MEMBERS: [&str; 7] = [
    "color.pal",
    "data/vault13.gam",
    "maps/artemple.map",
    "data/maps.txt",
    "data/city.txt",
    "data/worldmap.txt",
    "data/party.txt",
];

/// §2.2: the required members the master database, as mounted, lacks.
pub fn missing_members(provider: &Dat2Provider) -> Vec<&'static str> {
    REQUIRED_MEMBERS
        .into_iter()
        .filter(|member| !provider.in_master(member))
        .collect()
}
