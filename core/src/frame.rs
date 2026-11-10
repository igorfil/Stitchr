use crate::{ComposeError, RgbColor};
use ab_glyph::{point, Font, FontRef, Glyph, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

const FONT_DATA: &[u8] = include_bytes!("../fonts/Inter-SemiBold.ttf");

/// A border around every image, with an optional header tab holding each image's label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecFrame {
    pub labels: Vec<String>,
    pub border_width: u32,
    pub border_color: RgbColor,
    pub label_size: u32,
}

fn font() -> FontRef<'static> {
    FontRef::try_from_slice(FONT_DATA).expect("bundled font is a valid TrueType file")
}

fn scale(frame: &SpecFrame) -> PxScale {
    PxScale::from(frame.label_size as f32)
}

fn padding_x(frame: &SpecFrame) -> u32 {
    frame.label_size / 2
}

fn padding_y(frame: &SpecFrame) -> u32 {
    frame.label_size / 4
}

fn header_height(frame: &SpecFrame) -> u32 {
    let font = font();
    let scaled = font.as_scaled(scale(frame));
    (scaled.ascent() - scaled.descent()).ceil() as u32 + 2 * padding_y(frame)
}

fn tab_height(frame: &SpecFrame) -> u32 {
    frame.border_width + header_height(frame)
}

fn slant(frame: &SpecFrame) -> u32 {
    tab_height(frame) / 2
}

fn layout_text(text: &str, scale: PxScale, origin: (f32, f32)) -> (Vec<Glyph>, f32) {
    let font = font();
    let scaled = font.as_scaled(scale);
    let (glyphs, caret, _) = text.chars().fold(
        (Vec::new(), origin.0, None),
        |(mut glyphs, caret, previous), character| {
            let id = scaled.glyph_id(character);
            let caret = previous.map_or(caret, |previous| caret + scaled.kern(previous, id));
            glyphs.push(id.with_scale_and_position(scale, point(caret, origin.1)));
            (glyphs, caret + scaled.h_advance(id), Some(id))
        },
    );
    (glyphs, caret - origin.0)
}

fn text_width(frame: &SpecFrame, text: &str) -> u32 {
    layout_text(text, scale(frame), (0.0, 0.0)).1.ceil() as u32
}

fn tab_bottom_width(frame: &SpecFrame, label: &str, left_inset: u32) -> u32 {
    left_inset + 2 * padding_x(frame) + text_width(frame, label)
}

fn tab_top_width(frame: &SpecFrame, label: &str, left_inset: u32) -> u32 {
    tab_bottom_width(frame, label, left_inset) + slant(frame)
}

/// Offset of an image's top-left corner from its tile's top-left corner.
pub(crate) fn inset(frame: &SpecFrame) -> (u32, u32) {
    (frame.border_width, frame.border_width)
}

pub(crate) fn tile_size(
    frame: &SpecFrame,
    (width, height): (u32, u32),
) -> Result<(u32, u32), ComposeError> {
    let border = frame
        .border_width
        .checked_mul(2)
        .ok_or(ComposeError::SizeOverflow)?;
    width
        .checked_add(border)
        .zip(height.checked_add(border))
        .ok_or(ComposeError::SizeOverflow)
}

fn blend(canvas: &mut RgbaImage, (x, y): (i64, i64), color: RgbColor, coverage: f32) {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return;
    };
    if x >= canvas.width() || y >= canvas.height() {
        return;
    }
    let coverage = coverage.clamp(0.0, 1.0);
    let pixel = canvas.get_pixel_mut(x, y);
    let mix = |from: u8, to: u8| {
        (f32::from(from) * (1.0 - coverage) + f32::from(to) * coverage).round() as u8
    };
    *pixel = Rgba([
        mix(pixel.0[0], color.red),
        mix(pixel.0[1], color.green),
        mix(pixel.0[2], color.blue),
        255,
    ]);
}

fn fill_rect(
    canvas: &mut RgbaImage,
    (x, y): (i64, i64),
    (width, height): (u32, u32),
    color: RgbColor,
) {
    (0..i64::from(height))
        .flat_map(|dy| (0..i64::from(width)).map(move |dx| (x + dx, y + dy)))
        .for_each(|pixel| blend(canvas, pixel, color, 1.0));
}

fn text_color(background: RgbColor) -> RgbColor {
    let luminance = 299 * u32::from(background.red)
        + 587 * u32::from(background.green)
        + 114 * u32::from(background.blue);
    if luminance > 150_000 {
        RgbColor::default()
    } else {
        RgbColor {
            red: 255,
            green: 255,
            blue: 255,
        }
    }
}

fn draw_text(canvas: &mut RgbaImage, frame: &SpecFrame, text: &str, origin: (f32, f32)) {
    let font = font();
    let color = text_color(frame.border_color);
    let (glyphs, _) = layout_text(text, scale(frame), origin);
    glyphs
        .into_iter()
        .filter_map(|glyph| font.outline_glyph(glyph))
        .for_each(|outlined| {
            let bounds = outlined.px_bounds();
            outlined.draw(|x, y, coverage| {
                blend(
                    canvas,
                    (
                        bounds.min.x as i64 + i64::from(x),
                        bounds.min.y as i64 + i64::from(y),
                    ),
                    color,
                    coverage,
                );
            });
        });
}

/// Draws a tab whose top-left is `(x, y)`, with a "/" cut on its right edge.
/// `left_inset` reserves room for a border running down the tab's left side.
fn draw_tab(
    canvas: &mut RgbaImage,
    frame: &SpecFrame,
    label: &str,
    (x, y): (i64, i64),
    left_inset: u32,
) {
    let height = tab_height(frame);
    let top_width = tab_top_width(frame, label, left_inset);
    let slant = slant(frame) as f32;
    (0..height).for_each(|row| {
        let edge = top_width as f32 - slant * (row as f32 + 0.5) / height as f32;
        (0..top_width).for_each(|column| {
            let coverage = edge - column as f32;
            if coverage > 0.0 {
                blend(
                    canvas,
                    (x + i64::from(column), y + i64::from(row)),
                    frame.border_color,
                    coverage,
                );
            }
        });
    });
    let font = font();
    let ascent = font.as_scaled(scale(frame)).ascent();
    draw_text(
        canvas,
        frame,
        label,
        (
            (x + i64::from(left_inset + padding_x(frame))) as f32,
            (y + i64::from(frame.border_width + padding_y(frame))) as f32 + ascent,
        ),
    );
}

fn draw_border(
    canvas: &mut RgbaImage,
    frame: &SpecFrame,
    (x, y): (i64, i64),
    (width, height): (u32, u32),
) {
    let border = frame.border_width;
    let color = frame.border_color;
    fill_rect(canvas, (x, y), (width, border), color);
    fill_rect(
        canvas,
        (x, y + i64::from(height) - i64::from(border)),
        (width, border),
        color,
    );
    fill_rect(canvas, (x, y), (border, height), color);
    fill_rect(
        canvas,
        (x + i64::from(width) - i64::from(border), y),
        (border, height),
        color,
    );
}

/// Draws the border and label tab for the tile containing an image placed at `image_position`.
pub(crate) fn draw_tile(
    canvas: &mut RgbaImage,
    frame: &SpecFrame,
    label: &str,
    image_position: (u32, u32),
    image_size: (u32, u32),
) -> Result<(), ComposeError> {
    let (left, top) = inset(frame);
    let origin = (
        i64::from(image_position.0) - i64::from(left),
        i64::from(image_position.1) - i64::from(top),
    );
    draw_border(canvas, frame, origin, tile_size(frame, image_size)?);
    if !label.is_empty() {
        draw_tab(canvas, frame, label, origin, frame.border_width);
    }
    Ok(())
}

/// Draws one border around the whole split canvas and a tab per band.
/// `band_starts[k]` is the x offset, within the split image, where band `k` begins on its top row.
pub(crate) fn draw_split(canvas: &mut RgbaImage, frame: &SpecFrame, band_starts: &[u32]) {
    let (left, _) = inset(frame);
    draw_border(canvas, frame, (0, 0), (canvas.width(), canvas.height()));
    frame
        .labels
        .iter()
        .zip(band_starts)
        .enumerate()
        .filter(|(_, (label, _))| !label.is_empty())
        .for_each(|(index, (label, &start))| {
            if index == 0 {
                draw_tab(canvas, frame, label, (0, 0), frame.border_width);
            } else {
                draw_tab(canvas, frame, label, (i64::from(left + start), 0), 0);
            }
        });
}
