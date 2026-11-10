use crate::RgbColor;
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

/// A two-color background gradient. `angle` follows CSS: 0 points up, 90 right, 180 down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SpecGradient {
    Linear {
        from: RgbColor,
        to: RgbColor,
        angle: u32,
    },
    Radial {
        from: RgbColor,
        to: RgbColor,
    },
}

fn mix(from: RgbColor, to: RgbColor, t: f64) -> Rgba<u8> {
    let t = t.clamp(0.0, 1.0);
    let channel =
        |from: u8, to: u8| (f64::from(from) + (f64::from(to) - f64::from(from)) * t).round() as u8;
    Rgba([
        channel(from.red, to.red),
        channel(from.green, to.green),
        channel(from.blue, to.blue),
        255,
    ])
}

fn linear_position((x, y): (f64, f64), (width, height): (f64, f64), angle: u32) -> f64 {
    let radians = f64::from(angle % 360).to_radians();
    let (dx, dy) = (radians.sin(), -radians.cos());
    let length = (width * dx).abs() + (height * dy).abs();
    ((x - width / 2.0) * dx + (y - height / 2.0) * dy) / length + 0.5
}

fn radial_position((x, y): (f64, f64), (width, height): (f64, f64)) -> f64 {
    let radius = (width / 2.0).hypot(height / 2.0);
    (x - width / 2.0).hypot(y - height / 2.0) / radius
}

pub(crate) fn paint(canvas: &mut RgbaImage, gradient: &SpecGradient) {
    let size = (f64::from(canvas.width()), f64::from(canvas.height()));
    for (x, y, pixel) in canvas.enumerate_pixels_mut() {
        let center = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        *pixel = match *gradient {
            SpecGradient::Linear { from, to, angle } => {
                mix(from, to, linear_position(center, size, angle))
            }
            SpecGradient::Radial { from, to } => mix(from, to, radial_position(center, size)),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: RgbColor = RgbColor {
        red: 0,
        green: 0,
        blue: 0,
    };
    const WHITE: RgbColor = RgbColor {
        red: 255,
        green: 255,
        blue: 255,
    };

    fn painted(width: u32, height: u32, gradient: SpecGradient) -> RgbaImage {
        let mut canvas = RgbaImage::new(width, height);
        paint(&mut canvas, &gradient);
        canvas
    }

    #[test]
    fn default_linear_angle_runs_from_top_to_bottom() {
        let canvas = painted(
            4,
            100,
            SpecGradient::Linear {
                from: BLACK,
                to: WHITE,
                angle: 180,
            },
        );

        assert_eq!(canvas.get_pixel(0, 0).0, [1, 1, 1, 255]);
        assert_eq!(canvas.get_pixel(3, 0), canvas.get_pixel(0, 0));
        assert_eq!(canvas.get_pixel(0, 99).0, [254, 254, 254, 255]);
    }

    #[test]
    fn ninety_degree_linear_gradient_runs_from_left_to_right() {
        let canvas = painted(
            100,
            4,
            SpecGradient::Linear {
                from: BLACK,
                to: WHITE,
                angle: 90,
            },
        );

        assert_eq!(canvas.get_pixel(0, 3).0, [1, 1, 1, 255]);
        assert_eq!(canvas.get_pixel(99, 0).0, [254, 254, 254, 255]);
    }

    #[test]
    fn radial_gradient_starts_at_the_center_and_ends_at_the_corners() {
        let canvas = painted(
            101,
            101,
            SpecGradient::Radial {
                from: BLACK,
                to: WHITE,
            },
        );

        assert_eq!(canvas.get_pixel(50, 50).0, [0, 0, 0, 255]);
        assert!(canvas.get_pixel(0, 0).0[0] > 250);
        assert_eq!(canvas.get_pixel(0, 0), canvas.get_pixel(100, 100));
    }
}
