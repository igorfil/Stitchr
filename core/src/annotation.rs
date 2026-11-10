use crate::{ComposeError, RgbColor, SpecPosition};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnnotationShape {
    /// A rectangle outline whose outer edge is `x`, `y`, `width`, `height`.
    Box {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    /// A line from `from` to `to` with an arrowhead at `to`.
    Arrow {
        from: SpecPosition,
        to: SpecPosition,
    },
}

/// A shape drawn on top of the finished composition, in output canvas pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecAnnotation {
    #[serde(flatten)]
    pub shape: AnnotationShape,
    pub color: RgbColor,
    pub line_width: u32,
}

impl AnnotationShape {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Box { .. } => "box",
            Self::Arrow { .. } => "arrow",
        }
    }
}

/// An annotation that reaches past the canvas edge, so part or all of it is not drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationClipped {
    pub index: usize,
    pub kind: &'static str,
    /// Whether any of it is still on the canvas.
    pub visible: bool,
}

fn invalid_reason(annotation: &SpecAnnotation) -> Option<&'static str> {
    match annotation.shape {
        _ if annotation.line_width == 0 => Some("line_width must be positive"),
        AnnotationShape::Box { width, height, .. } if width == 0 || height == 0 => {
            Some("box width and height must be positive")
        }
        AnnotationShape::Arrow { ref from, ref to } if from == to => {
            Some("arrow start and end must differ")
        }
        _ => None,
    }
}

pub fn validate_annotations(annotations: &[SpecAnnotation]) -> Result<(), ComposeError> {
    annotations
        .iter()
        .enumerate()
        .find_map(|(index, annotation)| {
            invalid_reason(annotation)
                .map(|reason| ComposeError::InvalidAnnotation { index, reason })
        })
        .map_or(Ok(()), Err)
}

/// Whether the segment touches the canvas `[0, width] x [0, height]` (Liang-Barsky).
fn segment_meets_canvas(from: (f64, f64), to: (f64, f64), (width, height): (f64, f64)) -> bool {
    let delta = (to.0 - from.0, to.1 - from.1);
    let edges = [
        (-delta.0, from.0),
        (delta.0, width - from.0),
        (-delta.1, from.1),
        (delta.1, height - from.1),
    ];
    let range = edges
        .iter()
        .try_fold((0.0_f64, 1.0_f64), |(start, end), &(p, q)| {
            if p == 0.0 {
                return (q >= 0.0).then_some((start, end));
            }
            let t = q / p;
            let range = if p < 0.0 {
                (start.max(t), end)
            } else {
                (start, end.min(t))
            };
            (range.0 <= range.1).then_some(range)
        });
    range.is_some()
}

fn clipping(annotation: &SpecAnnotation, (width, height): (u32, u32)) -> Option<bool> {
    let inside = |x: u32, y: u32| x < width && y < height;
    match annotation.shape {
        AnnotationShape::Box {
            x,
            y,
            width: box_width,
            height: box_height,
        } => {
            let fits = u64::from(x) + u64::from(box_width) <= u64::from(width)
                && u64::from(y) + u64::from(box_height) <= u64::from(height);
            (!fits).then_some(inside(x, y))
        }
        AnnotationShape::Arrow { ref from, ref to } => {
            let fits = inside(from.x, from.y) && inside(to.x, to.y);
            (!fits).then(|| {
                segment_meets_canvas(
                    (f64::from(from.x), f64::from(from.y)),
                    (f64::from(to.x), f64::from(to.y)),
                    (f64::from(width), f64::from(height)),
                )
            })
        }
    }
}

/// Annotations that extend past a `size` canvas.
pub fn annotation_warnings(
    annotations: &[SpecAnnotation],
    size: (u32, u32),
) -> Vec<AnnotationClipped> {
    annotations
        .iter()
        .enumerate()
        .filter_map(|(index, annotation)| {
            clipping(annotation, size).map(|visible| AnnotationClipped {
                index,
                kind: annotation.shape.kind(),
                visible,
            })
        })
        .collect()
}

fn pixel_range(low: f64, high: f64, limit: u32) -> std::ops::Range<u32> {
    let clamp = |value: f64| value.clamp(0.0, f64::from(limit)) as u32;
    clamp(low.floor())..clamp(high.ceil())
}

fn draw_box(
    canvas: &mut RgbaImage,
    (x, y, width, height): (u32, u32, u32, u32),
    line: u32,
    color: Rgba<u8>,
) {
    let (x, y, width, height, line) = (
        u64::from(x),
        u64::from(y),
        u64::from(width),
        u64::from(height),
        u64::from(line),
    );
    let x_end = (x + width).min(u64::from(canvas.width()));
    let y_end = (y + height).min(u64::from(canvas.height()));
    for py in y..y_end {
        for px in x..x_end {
            let on_stroke =
                px < x + line || px + line >= x + width || py < y + line || py + line >= y + height;
            if on_stroke {
                canvas.put_pixel(px as u32, py as u32, color);
            }
        }
    }
}

type Point = (f64, f64);

fn distance_to_segment(point: Point, start: Point, end: Point) -> f64 {
    let segment = (end.0 - start.0, end.1 - start.1);
    let length_squared = segment.0 * segment.0 + segment.1 * segment.1;
    let t = if length_squared == 0.0 {
        0.0
    } else {
        (((point.0 - start.0) * segment.0 + (point.1 - start.1) * segment.1) / length_squared)
            .clamp(0.0, 1.0)
    };
    let nearest = (start.0 + t * segment.0, start.1 + t * segment.1);
    (point.0 - nearest.0).hypot(point.1 - nearest.1)
}

fn in_triangle(point: Point, [a, b, c]: [Point; 3]) -> bool {
    let side = |p: Point, q: Point| (q.0 - p.0) * (point.1 - p.1) - (q.1 - p.1) * (point.0 - p.0);
    let signs = [side(a, b), side(b, c), side(c, a)];
    signs.iter().all(|&s| s >= 0.0) || signs.iter().all(|&s| s <= 0.0)
}

fn draw_arrow(canvas: &mut RgbaImage, from: Point, to: Point, line: u32, color: Rgba<u8>) {
    let line = f64::from(line);
    let length = (to.0 - from.0).hypot(to.1 - from.1);
    let direction = ((to.0 - from.0) / length, (to.1 - from.1) / length);
    let head_length = (4.0 * line).max(12.0).min(length);
    let half_head = head_length / 2.0;
    let base = (
        to.0 - direction.0 * head_length,
        to.1 - direction.1 * head_length,
    );
    let head = [
        to,
        (
            base.0 - direction.1 * half_head,
            base.1 + direction.0 * half_head,
        ),
        (
            base.0 + direction.1 * half_head,
            base.1 - direction.0 * half_head,
        ),
    ];
    let points = [from, head[0], head[1], head[2]];
    let reach = line / 2.0;
    let (min_x, max_x, min_y, max_y) = points.iter().fold(
        (f64::MAX, f64::MIN, f64::MAX, f64::MIN),
        |(min_x, max_x, min_y, max_y), &(x, y)| {
            (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y))
        },
    );
    for py in pixel_range(min_y - reach, max_y + reach, canvas.height()) {
        for px in pixel_range(min_x - reach, max_x + reach, canvas.width()) {
            let center = (f64::from(px) + 0.5, f64::from(py) + 0.5);
            if distance_to_segment(center, from, base) <= reach || in_triangle(center, head) {
                canvas.put_pixel(px, py, color);
            }
        }
    }
}

fn pixel_center(point: &SpecPosition) -> Point {
    (f64::from(point.x) + 0.5, f64::from(point.y) + 0.5)
}

/// Draws `annotations` in order; parts outside the canvas are clipped.
pub(crate) fn draw(canvas: &mut RgbaImage, annotations: &[SpecAnnotation]) {
    for annotation in annotations {
        let color = Rgba([
            annotation.color.red,
            annotation.color.green,
            annotation.color.blue,
            255,
        ]);
        match &annotation.shape {
            AnnotationShape::Box {
                x,
                y,
                width,
                height,
            } => draw_box(
                canvas,
                (*x, *y, *width, *height),
                annotation.line_width,
                color,
            ),
            AnnotationShape::Arrow { from, to } => draw_arrow(
                canvas,
                pixel_center(from),
                pixel_center(to),
                annotation.line_width,
                color,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: RgbColor = RgbColor {
        red: 255,
        green: 0,
        blue: 0,
    };
    const PAINT: [u8; 4] = [255, 0, 0, 255];
    const BLANK: [u8; 4] = [0, 0, 0, 255];

    fn blank(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(BLANK))
    }

    fn rectangle(x: u32, y: u32, width: u32, height: u32, line_width: u32) -> SpecAnnotation {
        SpecAnnotation {
            shape: AnnotationShape::Box {
                x,
                y,
                width,
                height,
            },
            color: RED,
            line_width,
        }
    }

    fn arrow(from: (u32, u32), to: (u32, u32), line_width: u32) -> SpecAnnotation {
        SpecAnnotation {
            shape: AnnotationShape::Arrow {
                from: SpecPosition {
                    x: from.0,
                    y: from.1,
                },
                to: SpecPosition { x: to.0, y: to.1 },
            },
            color: RED,
            line_width,
        }
    }

    #[test]
    fn box_paints_an_outline_inside_its_bounds() {
        let mut canvas = blank(8, 8);
        draw(&mut canvas, &[rectangle(1, 2, 5, 4, 1)]);

        let painted = |x: u32, y: u32| {
            (1..6).contains(&x) && (2..6).contains(&y) && (x == 1 || x == 5 || y == 2 || y == 5)
        };
        for (x, y, pixel) in canvas.enumerate_pixels() {
            let expected = if painted(x, y) { PAINT } else { BLANK };
            assert_eq!(pixel.0, expected, "({x}, {y})");
        }
    }

    #[test]
    fn thick_box_outline_grows_inward() {
        let mut canvas = blank(10, 10);
        draw(&mut canvas, &[rectangle(0, 0, 10, 10, 3)]);

        assert_eq!(canvas.get_pixel(2, 5).0, PAINT);
        assert_eq!(canvas.get_pixel(3, 5).0, BLANK);
        assert_eq!(canvas.get_pixel(6, 5).0, BLANK);
        assert_eq!(canvas.get_pixel(7, 5).0, PAINT);
    }

    #[test]
    fn box_past_the_edge_is_clipped_without_panicking() {
        let mut canvas = blank(4, 4);
        draw(&mut canvas, &[rectangle(2, 2, u32::MAX, u32::MAX, 1)]);

        assert_eq!(canvas.get_pixel(2, 3).0, PAINT);
        assert_eq!(canvas.get_pixel(3, 3).0, BLANK);
    }

    #[test]
    fn arrow_paints_its_shaft_and_a_head_at_the_end() {
        let mut canvas = blank(60, 30);
        draw(&mut canvas, &[arrow((2, 15), (55, 15), 2)]);

        assert_eq!(canvas.get_pixel(5, 15).0, PAINT);
        assert_eq!(canvas.get_pixel(30, 15).0, PAINT);
        assert_eq!(canvas.get_pixel(5, 18).0, BLANK);
        assert_eq!(
            canvas.get_pixel(46, 19).0,
            PAINT,
            "head is wider than the shaft"
        );
        assert_eq!(canvas.get_pixel(55, 15).0, PAINT);
        assert_eq!(canvas.get_pixel(58, 15).0, BLANK);
    }

    #[test]
    fn arrow_shorter_than_its_head_is_only_a_head() {
        let mut canvas = blank(20, 20);
        draw(&mut canvas, &[arrow((10, 2), (10, 6), 4)]);

        assert_eq!(canvas.get_pixel(10, 5).0, PAINT);
        assert_eq!(canvas.get_pixel(10, 12).0, BLANK);
    }

    #[test]
    fn later_annotations_paint_over_earlier_ones() {
        let mut canvas = blank(4, 4);
        let mut blue = rectangle(0, 0, 4, 4, 4);
        blue.color = RgbColor {
            red: 0,
            green: 0,
            blue: 255,
        };
        draw(&mut canvas, &[rectangle(0, 0, 4, 4, 4), blue]);

        assert_eq!(canvas.get_pixel(1, 1).0, [0, 0, 255, 255]);
    }

    #[test]
    fn degenerate_annotations_are_rejected_with_their_index() {
        assert_eq!(
            validate_annotations(&[rectangle(0, 0, 1, 1, 1), rectangle(0, 0, 0, 3, 1)]),
            Err(ComposeError::InvalidAnnotation {
                index: 1,
                reason: "box width and height must be positive"
            })
        );
        assert_eq!(
            validate_annotations(&[arrow((3, 3), (3, 3), 1)]),
            Err(ComposeError::InvalidAnnotation {
                index: 0,
                reason: "arrow start and end must differ"
            })
        );
        assert_eq!(
            validate_annotations(&[arrow((0, 0), (3, 3), 0)]),
            Err(ComposeError::InvalidAnnotation {
                index: 0,
                reason: "line_width must be positive"
            })
        );
        assert_eq!(validate_annotations(&[]), Ok(()));
    }

    #[test]
    fn annotations_inside_the_canvas_do_not_warn() {
        assert_eq!(
            annotation_warnings(
                &[rectangle(0, 0, 10, 10, 1), arrow((0, 0), (9, 9), 1)],
                (10, 10)
            ),
            []
        );
    }

    #[test]
    fn annotations_past_the_edge_report_whether_any_part_shows() {
        assert_eq!(
            annotation_warnings(
                &[
                    rectangle(5, 5, 6, 1, 1),
                    rectangle(10, 0, 1, 1, 1),
                    arrow((5, 5), (20, 5), 1),
                    arrow((12, 0), (20, 20), 1),
                ],
                (10, 10)
            ),
            [
                AnnotationClipped {
                    index: 0,
                    kind: "box",
                    visible: true
                },
                AnnotationClipped {
                    index: 1,
                    kind: "box",
                    visible: false
                },
                AnnotationClipped {
                    index: 2,
                    kind: "arrow",
                    visible: true
                },
                AnnotationClipped {
                    index: 3,
                    kind: "arrow",
                    visible: false
                },
            ]
        );
    }

    #[test]
    fn annotations_round_trip_through_json() {
        let annotations = vec![rectangle(1, 2, 3, 4, 5), arrow((1, 2), (3, 4), 2)];
        let value = serde_json::to_value(&annotations).unwrap();

        assert_eq!(value[0]["type"], "box");
        assert_eq!(value[0]["width"], 3);
        assert_eq!(value[0]["line_width"], 5);
        assert_eq!(value[1]["type"], "arrow");
        assert_eq!(value[1]["to"]["y"], 4);
        assert_eq!(
            serde_json::from_value::<Vec<SpecAnnotation>>(value).unwrap(),
            annotations
        );
    }
}
