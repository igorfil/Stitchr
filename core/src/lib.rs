mod layout;

pub use image::DynamicImage;
pub use layout::{Alignment, ComposeError, ComposeOptions, Layout, StackDirection};

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

    let mut canvas = RgbaImage::new(plan.width, plan.height);
    for (image, (x, y)) in images.iter().zip(plan.positions) {
        imageops::overlay(&mut canvas, image, i64::from(x), i64::from(y));
    }

    let composed = DynamicImage::ImageRgba8(canvas);
    match options.max_size {
        Some(max_size) if plan.width.max(plan.height) > max_size => {
            Ok(composed.resize(max_size, max_size, imageops::FilterType::Lanczos3))
        }
        _ => Ok(composed),
    }
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
            max_size: None,
        };

        let result = compose(&images, &options).unwrap();

        assert_eq!(result.dimensions(), (6, 6));
        assert_eq!(result.get_pixel(2, 0).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(4, 1).0, [0, 255, 0, 255]);
        assert_eq!(result.get_pixel(0, 4).0, [0, 0, 255, 255]);
        assert_eq!(result.get_pixel(5, 3).0, [255, 255, 0, 255]);
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(result.get_pixel(3, 4).0, [0, 0, 0, 0]);
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
            max_size: None,
        };

        let result = compose(&images, &options).unwrap();

        assert_eq!(result.dimensions(), (5, 3));
        assert_eq!(result.get_pixel(0, 1).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(4, 0).0, [0, 0, 255, 255]);
        assert_eq!(result.get_pixel(2, 1).0, [0, 0, 0, 0]);
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 0, 0]);
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
            max_size: None,
        };

        let result = compose(&images, &options).unwrap();

        assert_eq!(result.dimensions(), (3, 4));
        assert_eq!(result.get_pixel(2, 0).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(0, 3).0, [0, 0, 255, 255]);
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(result.get_pixel(2, 2).0, [0, 0, 0, 0]);
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
            max_size: None,
        };

        let result = compose(&images, &options).unwrap();

        assert_eq!(result.dimensions(), (6, 3));
        assert_eq!(result.get_pixel(1, 1).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(2, 2).0, [0, 255, 0, 255]);
        assert_eq!(result.get_pixel(5, 0).0, [0, 0, 255, 255]);
        assert_eq!(result.get_pixel(0, 0).0, [0, 0, 0, 0]);
    }

    #[test]
    fn max_size_limits_longest_side_without_upscaling() {
        let images = [solid(4, 2, [255, 0, 0, 255]), solid(4, 2, [0, 0, 255, 255])];
        let options = ComposeOptions {
            layout: Layout::Stack {
                direction: StackDirection::Horizontal,
            },
            max_size: Some(4),
            ..ComposeOptions::default()
        };

        assert_eq!(compose(&images, &options).unwrap().dimensions(), (4, 1));
        let larger = ComposeOptions {
            max_size: Some(10),
            ..options
        };
        assert_eq!(compose(&images, &larger).unwrap().dimensions(), (8, 2));
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

        let invalid_max_size = ComposeOptions {
            max_size: Some(0),
            ..ComposeOptions::default()
        };
        assert_eq!(
            compose(&images, &invalid_max_size).err(),
            Some(ComposeError::InvalidMaxSize)
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
    fn png_roundtrip_preserves_alpha() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output.png");
        let original =
            compose(&[solid(2, 2, [255, 0, 0, 128])], &ComposeOptions::default()).unwrap();

        export_image(&original, &path, ExportFormat::Png).unwrap();
        let loaded = load_image(&path).unwrap();

        assert_eq!(loaded.dimensions(), (2, 2));
        assert_eq!(loaded.get_pixel(0, 0).0, [255, 0, 0, 128]);
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
