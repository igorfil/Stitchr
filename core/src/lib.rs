pub mod color;
pub mod ffi;
mod layout;
pub mod spec;

uniffi::setup_scaffolding!();

pub use color::{ColorParseError, RgbColor};
pub use image::DynamicImage;
pub use layout::{Alignment, ComposeError, ComposeOptions, Layout, StackDirection};
pub use spec::{plan_spec, CompositionSpec, PlacementPlan, SpecLayout, SpecPosition};

use image::{imageops, ImageFormat, Rgba, RgbaImage};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
}

pub fn load_image(path: impl AsRef<Path>) -> image::ImageResult<DynamicImage> {
    image::open(path)
}

pub fn compose(
    images: &[DynamicImage],
    options: &ComposeOptions,
) -> Result<DynamicImage, ComposeError> {
    let sizes: Vec<_> = images
        .iter()
        .map(|image| (image.width(), image.height()))
        .collect();
    let plan = layout::plan(&sizes, options)?;

    render_plan(
        images,
        &PlacementPlan {
            positions: plan.positions,
            width: plan.width,
            height: plan.height,
            background_color: RgbColor::default(),
        },
    )
}

pub fn render_plan(
    images: &[DynamicImage],
    plan: &PlacementPlan,
) -> Result<DynamicImage, ComposeError> {
    if plan.positions.len() != images.len() {
        return Err(ComposeError::ImageCountMismatch {
            expected: plan.positions.len(),
            actual: images.len(),
        });
    }
    let color = plan.background_color;
    let mut canvas = RgbaImage::from_pixel(
        plan.width,
        plan.height,
        Rgba([color.red, color.green, color.blue, 255]),
    );
    for (image, &(x, y)) in images.iter().zip(&plan.positions) {
        imageops::overlay(&mut canvas, image, i64::from(x), i64::from(y));
    }
    // `imageops::overlay` can round an opaque destination's alpha down to 254.
    for pixel in canvas.pixels_mut() {
        pixel.0[3] = 255;
    }

    Ok(DynamicImage::ImageRgba8(canvas))
}

pub fn render_spec(
    spec: &CompositionSpec,
    images: &[DynamicImage],
) -> Result<DynamicImage, ComposeError> {
    let dimensions = images
        .iter()
        .map(|image| (image.width(), image.height()))
        .collect::<Vec<_>>();
    let plan = plan_spec(spec, &dimensions)?;
    render_plan(images, &plan)
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("cannot load {path}: {source}")]
    Input {
        path: String,
        source: image::ImageError,
    },
    #[error(transparent)]
    Composition(#[from] ComposeError),
}

pub fn render_paths(spec: &CompositionSpec) -> Result<DynamicImage, RenderError> {
    let images = spec
        .paths
        .iter()
        .map(|path| {
            load_image(path).map_err(|source| RenderError::Input {
                path: path.clone(),
                source,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    render_spec(spec, &images).map_err(RenderError::Composition)
}

pub fn export_image(
    image: &DynamicImage,
    path: impl AsRef<Path>,
    format: ExportFormat,
) -> image::ImageResult<()> {
    match format {
        ExportFormat::Png => image.save_with_format(path, ImageFormat::Png),
        ExportFormat::Jpeg => {
            let mut background =
                RgbaImage::from_pixel(image.width(), image.height(), Rgba([255, 255, 255, 255]));
            imageops::overlay(&mut background, image, 0, 0);
            DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(background).to_rgb8())
                .save_with_format(path, ImageFormat::Jpeg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    fn solid(width: u32, height: u32, color: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, Rgba(color)))
    }

    #[test]
    fn renderer_paints_an_explicit_plan_without_planning() {
        let plan = PlacementPlan {
            positions: vec![(0, 0), (3, 1)],
            width: 4,
            height: 2,
            background_color: RgbColor {
                red: 12,
                green: 34,
                blue: 56,
            },
        };
        let images = [solid(1, 1, [255, 0, 0, 255]), solid(1, 1, [0, 0, 255, 255])];

        let rendered = render_plan(&images, &plan).unwrap();
        assert_eq!(rendered.dimensions(), (4, 2));
        assert_eq!(rendered.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(rendered.get_pixel(3, 1).0, [0, 0, 255, 255]);
        assert_eq!(rendered.get_pixel(1, 0).0, [12, 34, 56, 255]);
    }

    #[test]
    fn backend_renders_a_hand_written_spec_with_in_memory_images() {
        let spec = CompositionSpec {
            paths: vec![
                "no-files-needed-1.png".into(),
                "no-files-needed-2.png".into(),
            ],
            layout: SpecLayout::Stack {
                direction: StackDirection::Horizontal,
                spacing: 1,
                alignment: Alignment::Start,
            },
            background_color: RgbColor::default(),
        };

        let image = render_spec(
            &spec,
            &[solid(1, 1, [255, 0, 0, 255]), solid(1, 1, [0, 0, 255, 255])],
        )
        .unwrap();
        assert_eq!(image.dimensions(), (5, 3));
        assert_eq!(image.get_pixel(1, 1).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(3, 1).0, [0, 0, 255, 255]);
    }

    #[test]
    fn grid_positions_with_spacing_and_alignment() {
        let images = [
            solid(1, 2, [255, 0, 0, 255]),
            solid(2, 1, [0, 255, 0, 255]),
            solid(3, 2, [0, 0, 255, 255]),
            solid(1, 3, [255, 255, 0, 255]),
        ];
        let options = ComposeOptions {
            layout: Layout::Grid { columns: 2 },
            spacing: 1,
            alignment: Alignment::End,
        };

        let result = compose(&images, &options).unwrap();

        assert_eq!(result.dimensions(), (8, 8));
        assert_eq!(result.get_pixel(3, 1).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(5, 2).0, [0, 255, 0, 255]);
        assert_eq!(result.get_pixel(1, 5).0, [0, 0, 255, 255]);
        assert_eq!(result.get_pixel(6, 4).0, [255, 255, 0, 255]);
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(result.get_pixel(4, 5).0, [0, 0, 0, 255]);
    }

    #[test]
    fn horizontal_stack_positions_with_center_alignment() {
        let images = [solid(2, 1, [255, 0, 0, 255]), solid(1, 3, [0, 0, 255, 255])];
        let options = ComposeOptions {
            layout: Layout::Stack {
                direction: StackDirection::Horizontal,
            },
            spacing: 2,
            alignment: Alignment::Center,
        };

        let result = compose(&images, &options).unwrap();

        assert_eq!(result.dimensions(), (9, 7));
        assert_eq!(result.get_pixel(2, 3).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(6, 2).0, [0, 0, 255, 255]);
        assert_eq!(result.get_pixel(4, 3).0, [0, 0, 0, 255]);
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 0, 255]);
    }

    #[test]
    fn vertical_stack_positions_with_end_alignment() {
        let images = [solid(1, 2, [255, 0, 0, 255]), solid(3, 1, [0, 0, 255, 255])];
        let options = ComposeOptions {
            layout: Layout::Stack {
                direction: StackDirection::Vertical,
            },
            spacing: 1,
            alignment: Alignment::End,
        };

        let result = compose(&images, &options).unwrap();

        assert_eq!(result.dimensions(), (5, 6));
        assert_eq!(result.get_pixel(3, 1).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(1, 4).0, [0, 0, 255, 255]);
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(result.get_pixel(3, 3).0, [0, 0, 0, 255]);
    }

    #[test]
    fn one_image_without_options_has_no_border_and_opaque_black_background() {
        let image = solid(2, 1, [0, 0, 0, 0]);
        let result = compose(&[image], &ComposeOptions::default()).unwrap();

        assert_eq!(result.dimensions(), (2, 1));
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 0, 255]);
    }

    #[test]
    fn one_image_with_spacing_has_equal_black_border_on_all_sides() {
        let image = solid(2, 1, [255, 0, 0, 255]);
        let options = ComposeOptions {
            spacing: 20,
            ..ComposeOptions::default()
        };

        let result = compose(&[image], &options).unwrap();

        assert_eq!(result.dimensions(), (42, 41));
        assert_eq!(result.get_pixel(20, 20).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(21, 20).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(19, 20).0, [0, 0, 0, 255]);
        assert_eq!(result.get_pixel(22, 20).0, [0, 0, 0, 255]);
        assert_eq!(result.get_pixel(20, 19).0, [0, 0, 0, 255]);
        assert_eq!(result.get_pixel(20, 21).0, [0, 0, 0, 255]);
    }

    #[test]
    fn freeform_positions_and_input_order() {
        let images = [
            solid(2, 2, [255, 0, 0, 255]),
            solid(1, 1, [0, 255, 0, 255]),
            solid(1, 1, [0, 0, 255, 255]),
        ];
        let options = ComposeOptions {
            layout: Layout::Freeform {
                positions: vec![(1, 1), (2, 2), (5, 0)],
            },
            spacing: 100,
            alignment: Alignment::End,
        };

        let result = compose(&images, &options).unwrap();

        assert_eq!(result.dimensions(), (6, 3));
        assert_eq!(result.get_pixel(1, 1).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(2, 2).0, [0, 255, 0, 255]);
        assert_eq!(result.get_pixel(5, 0).0, [0, 0, 255, 255]);
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 0, 255]);
    }

    #[test]
    fn rejects_invalid_options_and_overflow() {
        let image = solid(1, 1, [255, 0, 0, 255]);
        let images = [image];

        assert_eq!(
            compose(&[], &ComposeOptions::default()).err(),
            Some(ComposeError::NoImages)
        );

        let invalid_grid = ComposeOptions {
            layout: Layout::Grid { columns: 0 },
            ..ComposeOptions::default()
        };
        assert_eq!(
            compose(&images, &invalid_grid).err(),
            Some(ComposeError::InvalidColumns)
        );

        let missing_positions = ComposeOptions {
            layout: Layout::Freeform { positions: vec![] },
            ..ComposeOptions::default()
        };
        assert_eq!(
            compose(&images, &missing_positions).err(),
            Some(ComposeError::FreeformPositionCount {
                expected: 1,
                actual: 0
            })
        );

        let overflowing_position = ComposeOptions {
            layout: Layout::Freeform {
                positions: vec![(u32::MAX, 0)],
            },
            ..ComposeOptions::default()
        };
        assert_eq!(
            compose(&images, &overflowing_position).err(),
            Some(ComposeError::SizeOverflow)
        );

        let empty_image = DynamicImage::ImageRgba8(RgbaImage::new(0, 1));
        assert_eq!(
            compose(&[empty_image], &ComposeOptions::default()).err(),
            Some(ComposeError::InvalidImageDimensions)
        );
    }

    #[test]
    fn png_roundtrip_blends_alpha_onto_black() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output.png");
        let original =
            compose(&[solid(2, 2, [255, 0, 0, 128])], &ComposeOptions::default()).unwrap();

        export_image(&original, &path, ExportFormat::Png).unwrap();
        let loaded = load_image(&path).unwrap();

        assert_eq!(loaded.dimensions(), (2, 2));
        assert_eq!(loaded.get_pixel(0, 0).0, [128, 0, 0, 255]);
    }

    #[test]
    fn jpeg_roundtrip_flattens_transparency_to_white() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output.jpg");
        let original = solid(1, 1, [0, 0, 0, 0]);

        export_image(&original, &path, ExportFormat::Jpeg).unwrap();
        let loaded = load_image(&path).unwrap();

        assert_eq!(loaded.dimensions(), (1, 1));
        assert_eq!(loaded.get_pixel(0, 0).0, [255, 255, 255, 255]);
    }
}
