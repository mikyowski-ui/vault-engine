//! SPEC-020 §2.2: the one world-map stepping, pointer and viewport path
//! that the Fallout 1 screens (`world`, SPEC-014 §3.39-§3.40) and the
//! Fallout 2 screens (`world2`, SPEC-019 §3.12-§3.13) share.
//!
//! - [`view`]: the world-map view state both editions keep (view origin,
//!   follow mode, pause and its grey, hover, travel dots, the held
//!   side-panel button and the pending step), its key handling, its pass
//!   gate and settling, and the edition's view bounds ([`Bounds`]), the one
//!   place the scroll geometry branches on the edition.
//! - [`screen`]: the [`WorldEdition`] hooks each edition implements, and
//!   the shared `MapPreview` world-map and town-map entry points built on
//!   them: keys, presses and releases, the timed pass, the cursor and the
//!   frame.
//! - [`paint`]: the drawing helpers both editions use: discovery
//!   darkening, travel dots, markers and labels, the red buttons, the date,
//!   the pause grey, the town-map triangles and the bar's indicator boxes.
//!
//! Presentation only: every rule here is the one the two screens applied
//! separately before (SPEC-020 §2.2 rule 1, `[Must-match]`).
mod paint;
mod screen;
mod view;

pub(crate) use paint::{VIEW_CLIP, draw_indicator_box, inside, red_button, triangle_art};
pub(crate) use screen::{Hold, Leg, TownPress, WorldEdition};
pub(crate) use view::{Bounds, WorldView};
