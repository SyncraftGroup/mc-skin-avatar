//! Image-level helpers for Minecraft skin textures.
//!
//! The crate works only on PNG bytes: it validates skin dimensions, detects the
//! slim (Alex) model, and renders head avatars. It performs no network I/O and
//! no 3D rendering, so it can be used from any Rust program.

use std::io::Cursor;

use image::{ImageFormat, RgbaImage, imageops};

/// Errors returned by the skin helpers.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The input is not a valid PNG image.
    #[error("skin is not a valid PNG: {0}")]
    Decode(#[from] image::ImageError),
    /// The skin dimensions are not a multiple of 64x32.
    #[error("unsupported skin size {0}x{1} (expected a multiple of 64x32)")]
    InvalidSize(u32, u32),
    /// The avatar PNG could not be encoded.
    #[error("failed to encode avatar")]
    Encode,
}

/// Validate a skin PNG and return its pixel dimensions.
///
/// Width must be a multiple of 64 and height a multiple of 32, so both the
/// legacy 64x32 layout and HD skins such as 128x128 are accepted.
pub fn dimensions(png: &[u8]) -> Result<(u32, u32), Error> {
    let image = image::load_from_memory_with_format(png, ImageFormat::Png)?;
    let (width, height) = (image.width(), image.height());
    validate_size(width, height)?;
    Ok((width, height))
}

fn validate_size(width: u32, height: u32) -> Result<(), Error> {
    if width < 64 || height < 32 || width % 64 != 0 || height % 32 != 0 {
        return Err(Error::InvalidSize(width, height));
    }
    Ok(())
}

/// Render the front of the head (with the hat overlay) as a square PNG.
///
/// `size` is the output edge length. The result is scaled with nearest-neighbour
/// so the pixel-art edges stay crisp.
pub fn avatar_png(skin_png: &[u8], size: u32) -> Result<Vec<u8>, Error> {
    let source = image::load_from_memory_with_format(skin_png, ImageFormat::Png)?.to_rgba8();
    let (width, height) = source.dimensions();
    validate_size(width, height)?;

    let scale = width / 64;
    let head = imageops::crop_imm(&source, 8 * scale, 8 * scale, 8 * scale, 8 * scale).to_image();

    let mut face = RgbaImage::new(8 * scale, 8 * scale);
    imageops::replace(&mut face, &head, 0, 0);

    // The hat layer sits at (40, 8) in both the 64x32 legacy and 64x64 layouts.
    if width >= 48 * scale && height >= 16 * scale {
        let hat =
            imageops::crop_imm(&source, 40 * scale, 8 * scale, 8 * scale, 8 * scale).to_image();
        blend_over(&mut face, &hat);
    }

    let avatar = imageops::resize(
        &face,
        size.max(1),
        size.max(1),
        imageops::FilterType::Nearest,
    );
    let mut out = Cursor::new(Vec::new());
    avatar
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|_| Error::Encode)?;
    Ok(out.into_inner())
}

/// Detect the `slim` (Alex) arm model from the skin pixels.
///
/// The left-arm region at `x = 46..48, y = 52..64` is empty on slim skins and
/// filled on classic skins. Only square skins (the newer layout) can be slim;
/// legacy 64x32 skins always return `false`.
pub fn detect_slim(skin_png: &[u8]) -> Result<bool, Error> {
    let image = image::load_from_memory_with_format(skin_png, ImageFormat::Png)?.to_rgba8();
    let (width, height) = image.dimensions();
    if width != height {
        return Ok(false);
    }
    let ratio = (width / 64).max(1);
    for x in 46 * ratio..48 * ratio {
        for y in 52 * ratio..64 * ratio {
            if x < width && y < height && image.get_pixel(x, y)[3] != 0 {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Render a flat (front-facing) head avatar: the face with the hat drawn as an
/// outer shell slightly larger than the head, so the two layers stay
/// distinguishable.
///
/// The result is a square `size x size` PNG with transparent corners.
pub fn render_avatar(skin_png: &[u8], size: u32) -> Result<Vec<u8>, Error> {
    let image = image::load_from_memory_with_format(skin_png, ImageFormat::Png)?.to_rgba8();
    let (width, height) = image.dimensions();
    validate_size(width, height)?;

    let ratio = (width / 64).max(1) as i64;
    let rf = ratio as f64;
    let projection = Projection::default();
    let (hidden, visible) = split_faces(&projection);

    let mut head: Vec<(Face, Quad)> = Vec::new();
    let mut helmet: Vec<(Face, Quad)> = Vec::new();

    let sample_px = |x: i64, y: i64| -> Option<[u8; 3]> {
        if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
            return None;
        }
        let pixel = image.get_pixel(x as u32, y as u32);
        if pixel[3] == 0 {
            None
        } else {
            Some([pixel[0], pixel[1], pixel[2]])
        }
    };
    let point = |x: f64, y: f64, z: f64| Point { x, y, z };

    // Head: an 8x8x8 cube spanning x/y 0..8 and z -2..6.
    for i in 0..8 * ratio {
        for j in 0..8 * ratio {
            let (x0, x1) = (i as f64, (i + 1) as f64);
            let (y0, y1) = (j as f64, (j + 1) as f64);
            head.push((
                Face::Back,
                Quad {
                    pts: [
                        point(x0, y0, -2.0 * rf),
                        point(x1, y0, -2.0 * rf),
                        point(x1, y1, -2.0 * rf),
                        point(x0, y1, -2.0 * rf),
                    ],
                    rgb: sample_px(32 * ratio - 1 - i, 8 * ratio + j),
                },
            ));
            head.push((
                Face::Front,
                Quad {
                    pts: [
                        point(x0, y0, 6.0 * rf),
                        point(x1, y0, 6.0 * rf),
                        point(x1, y1, 6.0 * rf),
                        point(x0, y1, 6.0 * rf),
                    ],
                    rgb: sample_px(8 * ratio + i, 8 * ratio + j),
                },
            ));
        }
    }
    for j in 0..8 * ratio {
        for k in -2 * ratio..6 * ratio {
            let (y0, y1) = (j as f64, (j + 1) as f64);
            let (z0, z1) = (k as f64, (k + 1) as f64);
            head.push((
                Face::Right,
                Quad {
                    pts: [
                        point(0.0, y0, z0),
                        point(0.0, y0, z1),
                        point(0.0, y1, z1),
                        point(0.0, y1, z0),
                    ],
                    rgb: sample_px(k + 2 * ratio, 8 * ratio + j),
                },
            ));
            head.push((
                Face::Left,
                Quad {
                    pts: [
                        point(8.0 * rf, y0, z0),
                        point(8.0 * rf, y0, z1),
                        point(8.0 * rf, y1, z1),
                        point(8.0 * rf, y1, z0),
                    ],
                    rgb: sample_px(22 * ratio - 1 - k, 8 * ratio + j),
                },
            ));
        }
    }
    for i in 0..8 * ratio {
        for k in -2 * ratio..6 * ratio {
            let (x0, x1) = (i as f64, (i + 1) as f64);
            let (z0, z1) = (k as f64, (k + 1) as f64);
            head.push((
                Face::Top,
                Quad {
                    pts: [
                        point(x0, 0.0, z0),
                        point(x1, 0.0, z0),
                        point(x1, 0.0, z1),
                        point(x0, 0.0, z1),
                    ],
                    rgb: sample_px(8 * ratio + i, k + 2 * ratio),
                },
            ));
            head.push((
                Face::Bottom,
                Quad {
                    pts: [
                        point(x0, 8.0 * rf, z0),
                        point(x1, 8.0 * rf, z0),
                        point(x1, 8.0 * rf, z1),
                        point(x0, 8.0 * rf, z1),
                    ],
                    rgb: sample_px(16 * ratio + i, 2 * ratio + k),
                },
            ));
        }
    }

    // Helmet: a 9x9x9 shell centered on the head (scaled by 9/8, offset by 0.5).
    let hx = |i: i64| i as f64 * 9.0 / 8.0 - 0.5 * rf;
    let hy = |j: i64| j as f64 * 9.0 / 8.0 - 0.5 * rf;
    let hz = |k: i64| {
        if k == -2 * ratio {
            -2.5 * rf
        } else if k == 6 * ratio {
            6.5 * rf
        } else {
            k as f64 * 9.0 / 8.0 - 0.5 * rf
        }
    };
    for i in 0..8 * ratio {
        for j in 0..8 * ratio {
            let (xs0, xs1) = (hx(i), hx(i + 1));
            let (ys0, ys1) = (hy(j), hy(j + 1));
            helmet.push((
                Face::Back,
                Quad {
                    pts: [
                        point(xs0, ys0, -2.5 * rf),
                        point(xs1, ys0, -2.5 * rf),
                        point(xs1, ys1, -2.5 * rf),
                        point(xs0, ys1, -2.5 * rf),
                    ],
                    rgb: sample_px(64 * ratio - 1 - i, 8 * ratio + j),
                },
            ));
            helmet.push((
                Face::Front,
                Quad {
                    pts: [
                        point(xs0, ys0, 6.5 * rf),
                        point(xs1, ys0, 6.5 * rf),
                        point(xs1, ys1, 6.5 * rf),
                        point(xs0, ys1, 6.5 * rf),
                    ],
                    rgb: sample_px(40 * ratio + i, 8 * ratio + j),
                },
            ));
        }
    }
    for j in 0..8 * ratio {
        for k in -2 * ratio..6 * ratio {
            let (ys0, ys1) = (hy(j), hy(j + 1));
            let (zs0, zs1) = (hz(k), hz(k + 1));
            helmet.push((
                Face::Right,
                Quad {
                    pts: [
                        point(-0.5 * rf, ys0, zs0),
                        point(-0.5 * rf, ys0, zs1),
                        point(-0.5 * rf, ys1, zs1),
                        point(-0.5 * rf, ys1, zs0),
                    ],
                    rgb: sample_px(34 * ratio + k, 8 * ratio + j),
                },
            ));
            helmet.push((
                Face::Left,
                Quad {
                    pts: [
                        point(8.5 * rf, ys0, zs0),
                        point(8.5 * rf, ys0, zs1),
                        point(8.5 * rf, ys1, zs1),
                        point(8.5 * rf, ys1, zs0),
                    ],
                    rgb: sample_px(54 * ratio - 1 - k, 8 * ratio + j),
                },
            ));
        }
    }
    for i in 0..8 * ratio {
        for k in -2 * ratio..6 * ratio {
            let (xs0, xs1) = (hx(i), hx(i + 1));
            let (zs0, zs1) = (hz(k), hz(k + 1));
            helmet.push((
                Face::Top,
                Quad {
                    pts: [
                        point(xs0, -0.5 * rf, zs0),
                        point(xs1, -0.5 * rf, zs0),
                        point(xs1, -0.5 * rf, zs1),
                        point(xs0, -0.5 * rf, zs1),
                    ],
                    rgb: sample_px(40 * ratio + i, k + 2 * ratio),
                },
            ));
            helmet.push((
                Face::Bottom,
                Quad {
                    pts: [
                        point(xs0, 8.5 * rf, zs0),
                        point(xs1, 8.5 * rf, zs0),
                        point(xs1, 8.5 * rf, zs1),
                        point(xs0, 8.5 * rf, zs1),
                    ],
                    rgb: sample_px(48 * ratio + i, 2 * ratio + k),
                },
            ));
        }
    }

    // Project every point once to find the bounding box.
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for piece in [&head, &helmet] {
        for (_, quad) in piece.iter() {
            for point in quad.pts.iter() {
                let (x, y, _) = projection.project(*point);
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    let ratio2 = 50.0;
    let canvas_width = (ratio2 * (max_x - min_x)) as i32 + 1;
    let canvas_height = (ratio2 * (max_y - min_y)) as i32 + 1;
    let mut canvas = vec![0u8; (canvas_width * canvas_height * 4) as usize];

    // Painter's order: hidden faces of both pieces, then visible faces.
    let order: [(&Vec<(Face, Quad)>, &Vec<Face>); 4] = [
        (&helmet, &hidden),
        (&head, &hidden),
        (&head, &visible),
        (&helmet, &visible),
    ];
    for (piece, faces) in order {
        for (face, quad) in piece.iter() {
            if !faces.contains(face) {
                continue;
            }
            let Some(rgb) = quad.rgb else { continue };

            let mut screen = [(0i32, 0i32); 4];
            let mut same_x = true;
            let mut same_y = true;
            for index in 0..4 {
                let (x, y, _) = projection.project(quad.pts[index]);
                screen[index] = (((x - min_x) * ratio2) as i32, ((y - min_y) * ratio2) as i32);
                if screen[index].0 != screen[0].0 {
                    same_x = false;
                }
                if screen[index].1 != screen[0].1 {
                    same_y = false;
                }
            }
            if same_x || same_y {
                continue;
            }
            fill_quad(&mut canvas, canvas_width, canvas_height, &screen, rgb);
        }
    }

    let rendered = RgbaImage::from_raw(canvas_width as u32, canvas_height as u32, canvas)
        .ok_or(Error::Encode)?;
    let half = imageops::resize(
        &rendered,
        (canvas_width / 2).max(1) as u32,
        (canvas_height / 2).max(1) as u32,
        imageops::FilterType::Triangle,
    );
    let avatar = imageops::resize(
        &half,
        size.max(1),
        size.max(1),
        imageops::FilterType::Triangle,
    );
    let mut out = Cursor::new(Vec::new());
    avatar
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|_| Error::Encode)?;
    Ok(out.into_inner())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Face {
    Back,
    Front,
    Right,
    Left,
    Top,
    Bottom,
}

const ALL_FACES: [Face; 6] = [
    Face::Back,
    Face::Front,
    Face::Right,
    Face::Left,
    Face::Top,
    Face::Bottom,
];

#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
    z: f64,
}

struct Quad {
    pts: [Point; 4],
    rgb: Option<[u8; 3]>,
}

#[derive(Clone, Copy)]
struct Projection {
    cos_alpha: f64,
    sin_alpha: f64,
    cos_omega: f64,
    sin_omega: f64,
}

impl Default for Projection {
    fn default() -> Self {
        // Flat front view: no yaw/pitch, only the face and the hat shell.
        let alpha = 0.0f64.to_radians();
        let omega = 0.0f64.to_radians();
        Self {
            cos_alpha: alpha.cos(),
            sin_alpha: alpha.sin(),
            cos_omega: omega.cos(),
            sin_omega: omega.sin(),
        }
    }
}

impl Projection {
    fn project(&self, point: Point) -> (f64, f64, f64) {
        let x = point.x;
        let y = point.y;
        let z = point.z;
        (
            x * self.cos_omega + z * self.sin_omega,
            x * self.sin_alpha * self.sin_omega + y * self.cos_alpha
                - z * self.sin_alpha * self.cos_omega,
            -x * self.cos_alpha * self.sin_omega
                + y * self.sin_alpha
                + z * self.cos_alpha * self.cos_omega,
        )
    }
}

/// Pick the hidden/visible face sets from the unit cube using the biggest
/// depth divergence, matching the classic isometric head view.
fn split_faces(projection: &Projection) -> (Vec<Face>, Vec<Face>) {
    let corners: [(Point, [Face; 3]); 8] = [
        (
            Point {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            [Face::Back, Face::Right, Face::Top],
        ),
        (
            Point {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            [Face::Front, Face::Right, Face::Top],
        ),
        (
            Point {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            [Face::Back, Face::Right, Face::Bottom],
        ),
        (
            Point {
                x: 0.0,
                y: 1.0,
                z: 1.0,
            },
            [Face::Front, Face::Right, Face::Bottom],
        ),
        (
            Point {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            [Face::Back, Face::Left, Face::Top],
        ),
        (
            Point {
                x: 1.0,
                y: 0.0,
                z: 1.0,
            },
            [Face::Front, Face::Left, Face::Top],
        ),
        (
            Point {
                x: 1.0,
                y: 1.0,
                z: 0.0,
            },
            [Face::Back, Face::Left, Face::Bottom],
        ),
        (
            Point {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            [Face::Front, Face::Left, Face::Bottom],
        ),
    ];

    let mut hidden_index = 0;
    let mut hidden_depth = f64::INFINITY;
    for (index, (point, _)) in corners.iter().enumerate() {
        let depth = projection.project(*point).2;
        if depth < hidden_depth {
            hidden_depth = depth;
            hidden_index = index;
        }
    }

    let hidden = corners[hidden_index].1.to_vec();
    let visible = ALL_FACES
        .iter()
        .copied()
        .filter(|face| !hidden.contains(face))
        .collect();
    (hidden, visible)
}

fn fill_quad(canvas: &mut [u8], width: i32, height: i32, points: &[(i32, i32); 4], rgb: [u8; 3]) {
    let mut y_min = points[0].1;
    let mut y_max = points[0].1;
    for point in points.iter().skip(1) {
        y_min = y_min.min(point.1);
        y_max = y_max.max(point.1);
    }
    y_min = y_min.max(0);
    y_max = y_max.min(height - 1);

    let mut crossings = Vec::with_capacity(4);
    for y in y_min..=y_max {
        let scan = y as f64 + 0.5;
        crossings.clear();
        for index in 0..4 {
            let a = points[index];
            let b = points[(index + 1) % 4];
            let (ay, by) = (a.1 as f64, b.1 as f64);
            if (ay <= scan && by > scan) || (by <= scan && ay > scan) {
                let t = (scan - ay) / (by - ay);
                crossings.push(a.0 as f64 + t * (b.0 as f64 - a.0 as f64));
            }
        }
        if crossings.len() < 2 {
            continue;
        }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut index = 0;
        while index + 1 < crossings.len() {
            let start = crossings[index].round() as i32;
            let end = crossings[index + 1].round() as i32;
            let start = start.max(0);
            let end = end.min(width - 1);
            for x in start..=end {
                let offset = ((y * width + x) * 4) as usize;
                canvas[offset] = rgb[0];
                canvas[offset + 1] = rgb[1];
                canvas[offset + 2] = rgb[2];
                canvas[offset + 3] = 255;
            }
            index += 2;
        }
    }
}

fn blend_over(target: &mut RgbaImage, layer: &RgbaImage) {
    for (x, y, pixel) in layer.enumerate_pixels() {
        let alpha = pixel[3] as u32;
        if alpha == 0 {
            continue;
        }
        if alpha == 255 {
            target.put_pixel(x, y, *pixel);
            continue;
        }

        let base = target.get_pixel(x, y);
        let a = alpha as f32 / 255.0;
        let blend = |top: u8, bottom: u8| -> u8 {
            ((top as f32 * a) + (bottom as f32 * (1.0 - a))).round() as u8
        };
        target.put_pixel(
            x,
            y,
            image::Rgba([
                blend(pixel[0], base[0]),
                blend(pixel[1], base[1]),
                blend(pixel[2], base[2]),
                ((alpha + base[3] as u32 * (255 - alpha) / 255).min(255)) as u8,
            ]),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn skin_with_face() -> Vec<u8> {
        let mut skin = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 0]));
        skin.put_pixel(8, 8, Rgba([200, 10, 10, 255]));
        skin.put_pixel(10, 10, Rgba([10, 200, 10, 255]));
        skin.put_pixel(11, 11, Rgba([200, 10, 10, 255]));
        // opaque hat pixel over the face corner
        skin.put_pixel(40, 8, Rgba([0, 0, 255, 255]));
        let mut out = Cursor::new(Vec::new());
        skin.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn renders_head_and_hat() {
        let avatar = avatar_png(&skin_with_face(), 16).unwrap();
        let image = image::load_from_memory(&avatar).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (16, 16));
        // 8 source pixels → 2 output pixels per axis at 16x16
        assert_eq!(*image.get_pixel(0, 0), Rgba([0, 0, 255, 255]));
        assert_eq!(*image.get_pixel(4, 4), Rgba([10, 200, 10, 255]));
        assert_eq!(*image.get_pixel(6, 6), Rgba([200, 10, 10, 255]));
    }

    #[test]
    fn detects_slim_from_arm_pixels() {
        // Fully opaque skin → the arm region has content, so it is classic.
        let opaque = RgbaImage::from_pixel(64, 64, Rgba([10, 20, 30, 255]));
        let mut out = Cursor::new(Vec::new());
        opaque.write_to(&mut out, ImageFormat::Png).unwrap();
        assert!(!detect_slim(&out.into_inner()).unwrap());

        // Empty arm region → slim.
        let mut empty_arm = RgbaImage::from_pixel(64, 64, Rgba([10, 20, 30, 255]));
        for x in 46..48 {
            for y in 52..64 {
                empty_arm.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
        let mut out = Cursor::new(Vec::new());
        empty_arm.write_to(&mut out, ImageFormat::Png).unwrap();
        assert!(detect_slim(&out.into_inner()).unwrap());

        // Legacy 64x32 skins cannot be slim.
        let legacy = RgbaImage::from_pixel(64, 32, Rgba([10, 20, 30, 255]));
        let mut out = Cursor::new(Vec::new());
        legacy.write_to(&mut out, ImageFormat::Png).unwrap();
        assert!(!detect_slim(&out.into_inner()).unwrap());
    }

    #[test]
    fn renders_square_flat_avatar() {
        let avatar = render_avatar(&skin_with_face(), 64).unwrap();
        let image = image::load_from_memory(&avatar).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (64, 64));
        assert!(image.pixels().any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn accepts_legacy_and_high_resolution() {
        assert!(dimensions(&skin_with_face()).is_ok());

        let mut hd = RgbaImage::from_pixel(128, 128, Rgba([0, 0, 0, 0]));
        hd.put_pixel(16, 16, Rgba([1, 2, 3, 255]));
        let mut out = Cursor::new(Vec::new());
        hd.write_to(&mut out, ImageFormat::Png).unwrap();
        let avatar = avatar_png(&out.into_inner(), 8).unwrap();
        assert_eq!(image::load_from_memory(&avatar).unwrap().width(), 8);

        assert!(dimensions(b"not a png").is_err());
    }
}
