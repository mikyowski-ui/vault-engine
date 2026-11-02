//! The application icon, drawn in code: a steel plate holding a round door
//! with a three-spoke handwheel and two hinges, in the launcher's steel and
//! amber. It is an original mark and owes nothing to any game's art.
//!
//! Plain Rust with no dependencies, so the desktop shell and `xtask` can both
//! include it by `#[path]`. Every shape is a signed distance in icon units
//! (the image spans -1 to 1 each way, y down), turned into coverage over one
//! pixel: the edges stay crisp at 256 and the shapes hold at 32, where the
//! finest details are left out.

type Rgb = [f32; 3];

/// The steel, lit from the top left.
const STEEL_LIGHT: Rgb = [112.0, 116.0, 122.0];
const STEEL_MID: Rgb = [70.0, 74.0, 80.0];
const STEEL_DARK: Rgb = [38.0, 41.0, 46.0];
const SHADOW: Rgb = [10.0, 10.0, 12.0];
const AMBER: Rgb = [226.0, 160.0, 46.0];
const AMBER_DARK: Rgb = [146.0, 98.0, 26.0];

/// The plate's half width and corner radius.
const PLATE: f32 = 0.94;
const PLATE_CORNER: f32 = 0.24;
/// The door's radius, and the amber ring and dark seam around it.
const DOOR: f32 = 0.66;
const SEAM: f32 = 0.71;
const RING: f32 = 0.84;
/// The handwheel's rim, hub and spokes.
const RIM: [f32; 2] = [0.34, 0.48];
const HUB: f32 = 0.14;
const SPOKE: f32 = 0.06;
/// Unit directions of the three spokes: straight up, then 120 degrees apart.
const SPOKES: [[f32; 2]; 3] = [[0.0, -1.0], [0.866_025_4, 0.5], [-0.866_025_4, 0.5]];
/// The hinges' centers' offset from the middle, on the door's right, and
/// their half sizes.
const HINGE: [f32; 2] = [0.74, 0.32];
const HINGE_SIZE: [f32; 2] = [0.15, 0.095];
/// The widest pixel, in icon units, that still draws the finest details
/// (a 64 pixel icon's).
const DETAIL: f32 = 2.0 / 64.0;

/// `size` by `size` RGBA pixels, row by row from the top, with a transparent
/// background. A `size` of 0 gives no pixels.
pub fn rgba(size: u32) -> Vec<u8> {
    let side = size as usize;
    let mut pixels = Vec::with_capacity(side * side * 4);
    let pixel = 2.0 / size.max(1) as f32;
    for y in 0..size {
        for x in 0..size {
            let center = |at: u32| (at as f32 + 0.5) * pixel - 1.0;
            pixels.extend(shade([center(x), center(y)], pixel));
        }
    }
    pixels
}

/// How much of a pixel `pixel` wide a shape covers, from the signed distance
/// to its edge at the pixel's center.
fn coverage(distance: f32, pixel: f32) -> f32 {
    (0.5 - distance / pixel).clamp(0.0, 1.0)
}

fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

fn circle(point: [f32; 2], center: [f32; 2], radius: f32) -> f32 {
    length(point[0] - center[0], point[1] - center[1]) - radius
}

/// A box centered on `center` with rounded corners.
fn rounded_box(point: [f32; 2], center: [f32; 2], half: [f32; 2], corner: f32) -> f32 {
    let x = (point[0] - center[0]).abs() - half[0] + corner;
    let y = (point[1] - center[1]).abs() - half[1] + corner;
    length(x.max(0.0), y.max(0.0)) + x.max(y).min(0.0) - corner
}

/// A band from the middle to the point `reach` along the unit `direction`,
/// with rounded ends.
fn capsule(point: [f32; 2], direction: [f32; 2], reach: f32, half_width: f32) -> f32 {
    let along = (point[0] * direction[0] + point[1] * direction[1]).clamp(0.0, reach);
    length(
        point[0] - direction[0] * along,
        point[1] - direction[1] * along,
    ) - half_width
}

fn mix(from: Rgb, to: Rgb, amount: f32) -> Rgb {
    let amount = amount.clamp(0.0, 1.0);
    [0, 1, 2].map(|axis| from[axis] + (to[axis] - from[axis]) * amount)
}

/// The wheel's rim, hub and spokes as one shape.
fn wheel(point: [f32; 2]) -> f32 {
    let radius = length(point[0], point[1]);
    let rim = (radius - (RIM[0] + RIM[1]) / 2.0).abs() - (RIM[1] - RIM[0]) / 2.0;
    let hub = radius - HUB;
    SPOKES
        .iter()
        .map(|direction| capsule(point, *direction, RIM[0] + 0.02, SPOKE))
        .fold(rim.min(hub), f32::min)
}

/// A straight-alpha pixel, built up from the back as premultiplied color.
struct Canvas {
    color: Rgb,
    alpha: f32,
}

impl Canvas {
    fn paint(&mut self, color: Rgb, alpha: f32) {
        let under = self.color;
        self.color = [0, 1, 2].map(|axis| color[axis] * alpha + under[axis] * (1.0 - alpha));
        self.alpha = alpha + self.alpha * (1.0 - alpha);
    }
    fn bytes(&self) -> [u8; 4] {
        if self.alpha <= 0.0 {
            return [0; 4];
        }
        let [r, g, b] = self.color.map(|value| (value / self.alpha).round() as u8);
        [r, g, b, (self.alpha * 255.0).round() as u8]
    }
}

/// One pixel's color at `point`.
fn shade(point: [f32; 2], pixel: f32) -> [u8; 4] {
    let [x, y] = point;
    let mut canvas = Canvas {
        color: [0.0; 3],
        alpha: 0.0,
    };
    // Top left is 0, bottom right is 1, across the whole icon.
    let light = (x + y + 2.0) / 4.0;
    let detail = pixel <= DETAIL;

    // The plate, with a lit rim on its top left and a dark one opposite.
    let plate = rounded_box(point, [0.0, 0.0], [PLATE; 2], PLATE_CORNER);
    canvas.paint(
        mix(STEEL_MID, STEEL_DARK, light * 1.4),
        coverage(plate, pixel),
    );
    let edge = (plate + 0.022).abs() - 0.022;
    canvas.paint(
        mix(STEEL_LIGHT, SHADOW, light * 1.5),
        coverage(edge, pixel) * coverage(plate, pixel),
    );

    // The corner rivets.
    if detail {
        for [dx, dy] in [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]] {
            let center = [dx * 0.79, dy * 0.79];
            canvas.paint(SHADOW, coverage(circle(point, center, 0.05), pixel));
            let head = [center[0] - 0.008, center[1] - 0.008];
            canvas.paint(STEEL_LIGHT, coverage(circle(point, head, 0.04), pixel));
        }
    }

    // The amber ring, the dark seam and the door.
    let radius = length(x, y);
    canvas.paint(
        mix(AMBER, AMBER_DARK, light * 1.5 - 0.25),
        coverage(radius - RING, pixel),
    );
    canvas.paint(SHADOW, coverage(radius - SEAM, pixel));
    let door = (x + y) / (2.0 * DOOR) + 0.5;
    if detail {
        canvas.paint(
            mix(STEEL_LIGHT, STEEL_DARK, door),
            coverage(radius - DOOR, pixel),
        );
        // A fine groove turned into the door's face.
        let groove = (radius - 0.57).abs() - 0.008;
        canvas.paint(SHADOW, coverage(groove, pixel) * 0.7);
    } else {
        // Darker when small, so the wheel stands out.
        canvas.paint(mix(STEEL_MID, SHADOW, door), coverage(radius - DOOR, pixel));
    }

    // The hinges bridge the seam on the right.
    for side in [-1.0, 1.0] {
        let center = [HINGE[0], HINGE[1] * side];
        canvas.paint(
            SHADOW,
            coverage(
                rounded_box(
                    [x - 0.015, y - 0.02],
                    center,
                    [HINGE_SIZE[0] + 0.015, HINGE_SIZE[1] + 0.015],
                    0.03,
                ),
                pixel,
            ),
        );
        canvas.paint(
            mix(
                STEEL_LIGHT,
                STEEL_MID,
                (y - center[1]) / HINGE_SIZE[1] / 2.0 + 0.5,
            ),
            coverage(rounded_box(point, center, HINGE_SIZE, 0.03), pixel),
        );
    }

    // The handwheel; a large one has a soft offset shadow and a center pin.
    if detail {
        canvas.paint(SHADOW, coverage(wheel([x - 0.03, y - 0.04]), pixel) * 0.8);
    }
    canvas.paint(
        mix(AMBER, AMBER_DARK, (x + y) / 0.96 + 0.5),
        coverage(wheel(point), pixel),
    );
    if detail {
        canvas.paint(SHADOW, coverage(radius - 0.075, pixel));
        canvas.paint(STEEL_LIGHT, coverage(radius - 0.05, pixel));
    }

    canvas.bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(image: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * size + x) * 4) as usize;
        [image[at], image[at + 1], image[at + 2], image[at + 3]]
    }

    #[test]
    fn every_size_has_its_four_bytes_a_pixel() {
        for size in [0, 1, 2, 3, 16, 32, 48, 64, 256] {
            assert_eq!(rgba(size).len(), (size * size * 4) as usize, "{size}");
        }
    }

    #[test]
    fn the_corners_are_transparent_and_the_middle_is_solid() {
        for size in [16, 32, 64, 256] {
            let image = rgba(size);
            let last = size - 1;
            for [x, y] in [[0, 0], [last, 0], [0, last], [last, last]] {
                assert_eq!(pixel(&image, size, x, y)[3], 0, "{size}: {x},{y}");
            }
            assert_eq!(pixel(&image, size, size / 2, size / 2)[3], 255, "{size}");
        }
    }

    #[test]
    fn it_is_the_same_each_time() {
        assert_eq!(rgba(32), rgba(32));
    }

    #[test]
    fn the_small_icon_keeps_its_amber_and_its_steel() {
        let image = rgba(32);
        let (pixels, _) = image.as_chunks::<4>();
        let count = |test: fn(&[u8; 4]) -> bool| pixels.iter().filter(|p| test(p)).count();
        // Amber: warm and opaque. Steel: neutral and opaque.
        let amber = count(|p| p[3] == 255 && p[0] > 120 && p[0] as u16 > p[2] as u16 * 2);
        let steel = count(|p| p[3] == 255 && p[0].abs_diff(p[2]) < 20 && p[0] > 30);
        assert!(amber > 32 * 32 / 16, "{amber}");
        assert!(steel > 32 * 32 / 8, "{steel}");
    }
}
