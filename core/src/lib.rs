pub mod color;
pub mod ffi;
mod layout;
pub mod spec;

uniffi::setup_scaffolding!();

pub use color::{ColorParseError, RgbColor};
pub use image::DynamicImage;
pub use layout::{Alignment, ComposeError, ComposeOptions, Layout, StackDirection};
pub use spec::{
    compose_spec, CompositionParams, CompositionSpec, ImageMetadata, ImagePlacement, PlacementPlan,
    SpecLayout, SpecPosition,
};

use image::{imageops, ImageDecoder, ImageFormat, ImageReader, Limits, Pixel, Rgba, RgbaImage};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
}

pub fn load_image(path: impl AsRef<Path>) -> image::ImageResult<DynamicImage> {
    let mut decoder = ImageReader::open(path)?.into_decoder()?;
    let mut limits = Limits::default();
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

#[derive(Debug, Clone)]
pub struct LoadedImage {
    pub image: DynamicImage,
    pub metadata: ImageMetadata,
}

pub fn load_image_with_metadata(path: impl AsRef<Path>) -> image::ImageResult<LoadedImage> {
    let path = path.as_ref();
    let image = load_image(path)?;
    let name = path
        .file_name()
        .unwrap_or_else(|| path.as_os_str())
        .to_string_lossy()
        .into_owned();
    let metadata = ImageMetadata {
        name,
        width: image.width(),
        height: image.height(),
    };
    Ok(LoadedImage { image, metadata })
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
    let background_color = RgbColor::default();
    if let Layout::Split { divider } = options.layout {
        return render_split(images, (plan.width, plan.height), divider, background_color);
    }

    render_plan(
        images,
        &PlacementPlan {
            positions: plan.positions,
            width: plan.width,
            height: plan.height,
            background_color,
        },
    )
}

fn new_canvas(width: u32, height: u32, color: RgbColor) -> Result<RgbaImage, ComposeError> {
    let length = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(ComposeError::SizeOverflow)?;
    Limits::default()
        .reserve_usize(length)
        .map_err(|_| ComposeError::SizeOverflow)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(length)
        .map_err(|_| ComposeError::SizeOverflow)?;
    pixels.resize(length, 0);
    let mut canvas =
        RgbaImage::from_raw(width, height, pixels).ok_or(ComposeError::SizeOverflow)?;
    for pixel in canvas.pixels_mut() {
        *pixel = Rgba([color.red, color.green, color.blue, 255]);
    }
    Ok(canvas)
}

fn render_split(
    images: &[DynamicImage],
    size: (u32, u32),
    divider: u32,
    background_color: RgbColor,
) -> Result<DynamicImage, ComposeError> {
    if images
        .iter()
        .any(|image| (image.width(), image.height()) != size)
    {
        return Err(ComposeError::SplitSizeMismatch);
    }
    let mut canvas = new_canvas(size.0, size.1, background_color)?;
    let layers = images
        .iter()
        .map(DynamicImage::to_rgba8)
        .collect::<Vec<_>>();
    for (x, y, pixel) in canvas.enumerate_pixels_mut() {
        if let Some(index) = layout::split_region((x, y), size, layers.len(), divider) {
            pixel.blend(layers[index].get_pixel(x, y));
            pixel.0[3] = 255;
        }
    }
    Ok(DynamicImage::ImageRgba8(canvas))
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
    let mut canvas = new_canvas(plan.width, plan.height, plan.background_color)?;
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
    if spec.images.len() != images.len() {
        return Err(ComposeError::ImageCountMismatch {
            expected: spec.images.len(),
            actual: images.len(),
        });
    }
    if let SpecLayout::Split { divider } = spec.layout {
        return render_split(
            images,
            (spec.width, spec.height),
            divider,
            spec.background_color,
        );
    }
    let plan = PlacementPlan {
        positions: spec
            .images
            .iter()
            .map(|image| (image.position.x, image.position.y))
            .collect(),
        width: spec.width,
        height: spec.height,
        background_color: spec.background_color,
    };
    render_plan(images, &plan)
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
    use image::{GenericImageView, ImageEncoder};

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const YELLOW: [u8; 4] = [255, 255, 0, 255];
    const MAGENTA: [u8; 4] = [255, 0, 255, 255];
    const CYAN: [u8; 4] = [0, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];
    const PURPLE: [u8; 4] = [102, 51, 153, 255];

    fn solid(width: u32, height: u32, color: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, Rgba(color)))
    }

    fn assert_canvas(
        image: &DynamicImage,
        width: u32,
        height: u32,
        expected_pixel: impl Fn(u32, u32) -> [u8; 4],
    ) {
        assert_eq!(image.dimensions(), (width, height));
        for y in 0..height {
            for x in 0..width {
                assert_eq!(
                    image.get_pixel(x, y).0,
                    expected_pixel(x, y),
                    "unexpected pixel at ({x}, {y})"
                );
            }
        }
    }

    fn assert_same_image(first: &DynamicImage, second: &DynamicImage) {
        assert_eq!(first.to_rgba8(), second.to_rgba8());
    }

    fn compose_with(
        images: &[DynamicImage],
        layout: Layout,
        spacing: u32,
        alignment: Alignment,
    ) -> DynamicImage {
        compose(
            images,
            &ComposeOptions {
                layout,
                spacing,
                alignment,
            },
        )
        .unwrap()
    }

    fn alignment_offset(alignment: Alignment, slack: u32) -> u32 {
        match alignment {
            Alignment::Start => 0,
            Alignment::Center => slack / 2,
            Alignment::End => slack,
        }
    }

    fn spec_for(
        images: &[DynamicImage],
        layout: SpecLayout,
        background_color: RgbColor,
    ) -> CompositionSpec {
        let metadata = images
            .iter()
            .enumerate()
            .map(|(index, image)| ImageMetadata {
                name: format!("image-{index}"),
                width: image.width(),
                height: image.height(),
            })
            .collect::<Vec<_>>();
        compose_spec(
            &metadata,
            &CompositionParams {
                layout,
                background_color,
            },
        )
        .unwrap()
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
    fn image_loader_returns_name_and_dimensions_with_pixels() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("source.png");
        export_image(&solid(7, 3, RED), &path, ExportFormat::Png).unwrap();

        let loaded = load_image_with_metadata(&path).unwrap();

        assert_eq!(loaded.metadata.name, "source.png");
        assert_eq!((loaded.metadata.width, loaded.metadata.height), (7, 3));
        assert_eq!(loaded.image.dimensions(), (7, 3));
    }

    #[test]
    fn image_loader_applies_all_exif_orientations_before_measuring() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oriented.jpg");
        let source =
            image::RgbImage::from_fn(3, 2, |x, y| image::Rgb([((y * 3 + x) * 50) as u8; 3]));
        for (orientation, dimensions, indices) in [
            (1, (3, 2), [0, 1, 2, 3, 4, 5]),
            (2, (3, 2), [2, 1, 0, 5, 4, 3]),
            (3, (3, 2), [5, 4, 3, 2, 1, 0]),
            (4, (3, 2), [3, 4, 5, 0, 1, 2]),
            (5, (2, 3), [0, 3, 1, 4, 2, 5]),
            (6, (2, 3), [3, 0, 4, 1, 5, 2]),
            (7, (2, 3), [5, 2, 4, 1, 3, 0]),
            (8, (2, 3), [2, 5, 1, 4, 0, 3]),
        ] {
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new(std::fs::File::create(&path).unwrap());
            encoder
                .set_exif_metadata(vec![
                    b'I',
                    b'I',
                    42,
                    0,
                    8,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0x12,
                    0x01,
                    3,
                    0,
                    1,
                    0,
                    0,
                    0,
                    orientation,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ])
                .unwrap();
            encoder.encode_image(&source).unwrap();
            drop(encoder);
            let raw = image::open(&path).unwrap();
            let loaded = load_image_with_metadata(&path).unwrap();

            assert_eq!((loaded.metadata.width, loaded.metadata.height), dimensions);
            assert_eq!(loaded.image.dimensions(), dimensions);
            let expected = indices.map(|index| raw.get_pixel(index % 3, index / 3));
            assert_eq!(
                loaded
                    .image
                    .to_rgba8()
                    .pixels()
                    .copied()
                    .collect::<Vec<_>>(),
                expected,
                "EXIF orientation {orientation}"
            );
        }
    }

    #[test]
    fn renderers_reject_canvas_overflow_and_excessive_allocations() {
        let images = [solid(1, 1, RED)];
        for (width, height) in [(u32::MAX, u32::MAX), (16_384, 16_384), (u32::MAX, 1)] {
            let plan = PlacementPlan {
                positions: vec![(0, 0)],
                width,
                height,
                background_color: RgbColor::default(),
            };
            assert_eq!(
                render_plan(&images, &plan).err(),
                Some(ComposeError::SizeOverflow)
            );

            let mut spec = spec_for(
                &images,
                SpecLayout::Grid {
                    columns: 1,
                    spacing: 0,
                    alignment: Alignment::Start,
                },
                RgbColor::default(),
            );
            spec.width = width;
            spec.height = height;
            assert_eq!(
                render_spec(&spec, &images).err(),
                Some(ComposeError::SizeOverflow)
            );
        }
    }

    #[test]
    fn backend_renders_a_hand_written_spec_with_in_memory_images() {
        let images = [solid(1, 1, RED), solid(1, 1, BLUE)];
        let spec = spec_for(
            &images,
            SpecLayout::Stack {
                direction: StackDirection::Horizontal,
                spacing: 1,
                alignment: Alignment::Start,
            },
            RgbColor::default(),
        );

        let image = render_spec(&spec, &images).unwrap();
        assert_eq!(image.dimensions(), (5, 3));
        assert_eq!(image.get_pixel(1, 1).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(3, 1).0, [0, 0, 255, 255]);
    }

    #[test]
    fn renderer_rejects_image_count_mismatch_in_composition_spec() {
        let images = [solid(1, 1, RED), solid(1, 1, BLUE)];
        let spec = spec_for(
            &images,
            SpecLayout::Grid {
                columns: 2,
                spacing: 0,
                alignment: Alignment::Start,
            },
            RgbColor::default(),
        );

        assert_eq!(
            render_spec(&spec, &images[..1]).err(),
            Some(ComposeError::ImageCountMismatch {
                expected: 2,
                actual: 1,
            })
        );
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
    fn repeated_input_path_is_rendered_twice() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("a.png");
        export_image(&solid(2, 1, RED), &input, ExportFormat::Png).unwrap();
        let loaded = [
            load_image_with_metadata(&input).unwrap(),
            load_image_with_metadata(&input).unwrap(),
        ];
        let metadata = loaded
            .iter()
            .map(|image| image.metadata.clone())
            .collect::<Vec<_>>();
        let spec = compose_spec(
            &metadata,
            &CompositionParams {
                layout: SpecLayout::Stack {
                    direction: StackDirection::Horizontal,
                    spacing: 0,
                    alignment: Alignment::Start,
                },
                background_color: RgbColor::default(),
            },
        )
        .unwrap();
        let images = loaded
            .into_iter()
            .map(|image| image.image)
            .collect::<Vec<_>>();

        let result = render_spec(&spec, &images).unwrap();

        assert_canvas(&result, 4, 1, |_, _| RED);
    }

    #[test]
    fn two_single_pixel_images_render_in_each_layout() {
        let images = [solid(1, 1, RED), solid(1, 1, BLUE)];

        let grid = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Grid { columns: 2 },
                spacing: 0,
                alignment: Alignment::Start,
            },
        )
        .unwrap();
        assert_canvas(&grid, 2, 1, |x, _| if x == 0 { RED } else { BLUE });

        let horizontal = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Stack {
                    direction: StackDirection::Horizontal,
                },
                spacing: 0,
                alignment: Alignment::Start,
            },
        )
        .unwrap();
        assert_canvas(&horizontal, 2, 1, |x, _| if x == 0 { RED } else { BLUE });

        let vertical = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Stack {
                    direction: StackDirection::Vertical,
                },
                spacing: 0,
                alignment: Alignment::Start,
            },
        )
        .unwrap();
        assert_canvas(&vertical, 1, 2, |_, y| if y == 0 { RED } else { BLUE });

        let freeform = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Freeform {
                    positions: vec![(0, 0), (0, 0)],
                },
                spacing: 0,
                alignment: Alignment::Start,
            },
        )
        .unwrap();
        assert_canvas(&freeform, 1, 1, |_, _| BLUE);
    }

    #[test]
    fn two_single_pixel_images_with_spacing_have_one_gap_and_equal_margins() {
        let images = [solid(1, 1, RED), solid(1, 1, BLUE)];
        let result = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Stack {
                    direction: StackDirection::Horizontal,
                },
                spacing: 20,
                alignment: Alignment::Start,
            },
        )
        .unwrap();

        assert_canvas(&result, 62, 41, |x, y| {
            if (x, y) == (20, 20) {
                RED
            } else if (x, y) == (41, 20) {
                BLUE
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn freeform_overlap_uses_last_image_and_canvas_fits_the_larger_image() {
        let forward = [solid(4, 4, RED), solid(2, 2, BLUE)];
        let reverse = [solid(2, 2, BLUE), solid(4, 4, RED)];
        let options = ComposeOptions {
            layout: Layout::Freeform {
                positions: vec![(0, 0), (0, 0)],
            },
            spacing: 0,
            alignment: Alignment::Start,
        };

        let blue_last = compose(&forward, &options).unwrap();
        assert_canvas(
            &blue_last,
            4,
            4,
            |x, y| {
                if x < 2 && y < 2 {
                    BLUE
                } else {
                    RED
                }
            },
        );

        let red_last = compose(&reverse, &options).unwrap();
        assert_canvas(&red_last, 4, 4, |_, _| RED);
    }

    #[test]
    fn freeform_ignores_spacing_and_alignment() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE)];
        let positions = vec![(1, 2), (5, 0)];
        let baseline = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Freeform {
                    positions: positions.clone(),
                },
                spacing: 0,
                alignment: Alignment::Start,
            },
        )
        .unwrap();
        let with_ignored_options = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Freeform { positions },
                spacing: 99,
                alignment: Alignment::End,
            },
        )
        .unwrap();

        assert_canvas(&baseline, 7, 4, |x, y| {
            if (1..5).contains(&x) && (2..4).contains(&y) {
                RED
            } else if (5..7).contains(&x) && y < 4 {
                BLUE
            } else {
                BLACK
            }
        });
        assert_same_image(&baseline, &with_ignored_options);
    }

    #[test]
    fn freeform_rejects_missing_and_extra_positions_for_two_images() {
        let images = [solid(1, 1, RED), solid(1, 1, BLUE)];

        let missing = ComposeOptions {
            layout: Layout::Freeform {
                positions: vec![(0, 0)],
            },
            ..ComposeOptions::default()
        };
        assert_eq!(
            compose(&images, &missing).err(),
            Some(ComposeError::FreeformPositionCount {
                expected: 2,
                actual: 1,
            })
        );

        let extra = ComposeOptions {
            layout: Layout::Freeform {
                positions: vec![(0, 0), (1, 1), (2, 2)],
            },
            ..ComposeOptions::default()
        };
        assert_eq!(
            compose(&images, &extra).err(),
            Some(ComposeError::FreeformPositionCount {
                expected: 2,
                actual: 3,
            })
        );
    }

    #[test]
    fn zero_dimension_image_is_rejected_even_with_a_valid_second_image() {
        let images = [
            solid(1, 1, RED),
            DynamicImage::ImageRgba8(RgbaImage::new(0, 3)),
        ];

        assert_eq!(
            compose(&images, &ComposeOptions::default()).err(),
            Some(ComposeError::InvalidImageDimensions)
        );
    }

    #[test]
    fn freeform_and_grid_report_size_overflow_for_two_images() {
        let images = [solid(1, 1, RED), solid(1, 1, BLUE)];
        let freeform = ComposeOptions {
            layout: Layout::Freeform {
                positions: vec![(0, 0), (u32::MAX, 0)],
            },
            ..ComposeOptions::default()
        };
        assert_eq!(
            compose(&images, &freeform).err(),
            Some(ComposeError::SizeOverflow)
        );

        let metadata = [
            ImageMetadata {
                name: "wide.png".into(),
                width: u32::MAX,
                height: 1,
            },
            ImageMetadata {
                name: "small.png".into(),
                width: 1,
                height: 1,
            },
        ];
        assert_eq!(
            compose_spec(
                &metadata,
                &CompositionParams {
                    layout: SpecLayout::Grid {
                        columns: 2,
                        spacing: 0,
                        alignment: Alignment::Start,
                    },
                    background_color: RgbColor::default(),
                }
            )
            .err(),
            Some(ComposeError::SizeOverflow)
        );
    }

    #[test]
    fn equal_sized_images_fit_grid_and_stack_layouts_without_spacing() {
        let images = [solid(3, 2, RED), solid(3, 2, BLUE)];
        let options = |layout| ComposeOptions {
            layout,
            spacing: 0,
            alignment: Alignment::Start,
        };

        let grid_two_columns = compose(&images, &options(Layout::Grid { columns: 2 })).unwrap();
        assert_canvas(
            &grid_two_columns,
            6,
            2,
            |x, _| {
                if x < 3 {
                    RED
                } else {
                    BLUE
                }
            },
        );

        let grid_one_column = compose(&images, &options(Layout::Grid { columns: 1 })).unwrap();
        assert_canvas(
            &grid_one_column,
            3,
            4,
            |_, y| {
                if y < 2 {
                    RED
                } else {
                    BLUE
                }
            },
        );

        let clamped_columns = compose(&images, &options(Layout::Grid { columns: 5 })).unwrap();
        assert_same_image(&grid_two_columns, &clamped_columns);

        let horizontal = compose(
            &images,
            &options(Layout::Stack {
                direction: StackDirection::Horizontal,
            }),
        )
        .unwrap();
        assert_same_image(&grid_two_columns, &horizontal);

        let vertical = compose(
            &images,
            &options(Layout::Stack {
                direction: StackDirection::Vertical,
            }),
        )
        .unwrap();
        assert_same_image(&grid_one_column, &vertical);
    }

    #[test]
    fn equal_sized_images_with_spacing_have_single_gap_and_outer_margins() {
        let images = [solid(3, 2, RED), solid(3, 2, BLUE)];
        let horizontal = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Stack {
                    direction: StackDirection::Horizontal,
                },
                spacing: 2,
                alignment: Alignment::Start,
            },
        )
        .unwrap();
        assert_canvas(&horizontal, 12, 6, |x, y| {
            if (2..5).contains(&x) && (2..4).contains(&y) {
                RED
            } else if (7..10).contains(&x) && (2..4).contains(&y) {
                BLUE
            } else {
                BLACK
            }
        });

        let grid = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Grid { columns: 2 },
                spacing: 2,
                alignment: Alignment::Start,
            },
        )
        .unwrap();
        assert_same_image(&horizontal, &grid);

        let vertical = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Stack {
                    direction: StackDirection::Vertical,
                },
                spacing: 2,
                alignment: Alignment::Start,
            },
        )
        .unwrap();
        assert_canvas(&vertical, 7, 10, |x, y| {
            if (2..5).contains(&x) && (2..4).contains(&y) {
                RED
            } else if (2..5).contains(&x) && (6..8).contains(&y) {
                BLUE
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn alignment_does_not_change_equal_sized_images() {
        let images = [solid(3, 2, RED), solid(3, 2, BLUE)];
        for layout in [
            Layout::Grid { columns: 2 },
            Layout::Stack {
                direction: StackDirection::Horizontal,
            },
            Layout::Stack {
                direction: StackDirection::Vertical,
            },
        ] {
            let start = compose(
                &images,
                &ComposeOptions {
                    layout: layout.clone(),
                    spacing: 0,
                    alignment: Alignment::Start,
                },
            )
            .unwrap();
            for alignment in [Alignment::Center, Alignment::End] {
                let result = compose(
                    &images,
                    &ComposeOptions {
                        layout: layout.clone(),
                        spacing: 0,
                        alignment,
                    },
                )
                .unwrap();
                assert_same_image(&start, &result);
            }
        }
    }

    #[test]
    fn horizontal_alignment_places_shorter_image_on_cross_axis() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE)];
        for (alignment, offset) in [
            (Alignment::Start, 0),
            (Alignment::Center, 1),
            (Alignment::End, 2),
        ] {
            let result = compose(
                &images,
                &ComposeOptions {
                    layout: Layout::Stack {
                        direction: StackDirection::Horizontal,
                    },
                    spacing: 0,
                    alignment,
                },
            )
            .unwrap();
            assert_canvas(&result, 6, 4, |x, y| {
                if x >= 4 {
                    BLUE
                } else if (offset..offset + 2).contains(&y) {
                    RED
                } else {
                    BLACK
                }
            });
        }
    }

    #[test]
    fn vertical_alignment_places_narrower_image_on_cross_axis() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE)];
        for alignment in [Alignment::Start, Alignment::Center, Alignment::End] {
            let offset = alignment_offset(alignment, 2);
            let result = compose(
                &images,
                &ComposeOptions {
                    layout: Layout::Stack {
                        direction: StackDirection::Vertical,
                    },
                    spacing: 0,
                    alignment,
                },
            )
            .unwrap();
            assert_canvas(&result, 4, 6, |x, y| {
                if y < 2 {
                    RED
                } else if (offset..offset + 2).contains(&x) {
                    BLUE
                } else {
                    BLACK
                }
            });
        }
    }

    #[test]
    fn center_alignment_uses_floor_for_odd_slack() {
        let images = [solid(3, 1, RED), solid(3, 4, BLUE)];
        let result = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Stack {
                    direction: StackDirection::Horizontal,
                },
                spacing: 0,
                alignment: Alignment::Center,
            },
        )
        .unwrap();

        assert_canvas(&result, 6, 4, |x, y| {
            if x >= 3 {
                BLUE
            } else if y == 1 {
                RED
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn swapping_image_order_moves_cross_axis_slack_with_the_smaller_image() {
        let images = [solid(2, 4, BLUE), solid(4, 2, RED)];
        for alignment in [Alignment::Start, Alignment::Center, Alignment::End] {
            let horizontal_offset = alignment_offset(alignment, 2);
            let horizontal = compose(
                &images,
                &ComposeOptions {
                    layout: Layout::Stack {
                        direction: StackDirection::Horizontal,
                    },
                    spacing: 0,
                    alignment,
                },
            )
            .unwrap();
            assert_canvas(&horizontal, 6, 4, |x, y| {
                if x < 2 {
                    BLUE
                } else if (horizontal_offset..horizontal_offset + 2).contains(&y) {
                    RED
                } else {
                    BLACK
                }
            });

            let vertical_offset = alignment_offset(alignment, 2);
            let vertical = compose(
                &images,
                &ComposeOptions {
                    layout: Layout::Stack {
                        direction: StackDirection::Vertical,
                    },
                    spacing: 0,
                    alignment,
                },
            )
            .unwrap();
            assert_canvas(&vertical, 4, 6, |x, y| {
                if y >= 4 {
                    RED
                } else if (vertical_offset..vertical_offset + 2).contains(&x) {
                    BLUE
                } else {
                    BLACK
                }
            });
        }
    }

    #[test]
    fn spacing_and_alignment_are_applied_inside_the_outer_margin() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE)];
        let result = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Stack {
                    direction: StackDirection::Horizontal,
                },
                spacing: 5,
                alignment: Alignment::Start,
            },
        )
        .unwrap();

        assert_canvas(&result, 21, 14, |x, y| {
            if (5..9).contains(&x) && (5..7).contains(&y) {
                RED
            } else if (14..16).contains(&x) && (5..9).contains(&y) {
                BLUE
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn two_column_grid_matches_horizontal_stack_for_every_alignment() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE)];
        for alignment in [Alignment::Start, Alignment::Center, Alignment::End] {
            let offset = alignment_offset(alignment, 2);
            let grid = compose(
                &images,
                &ComposeOptions {
                    layout: Layout::Grid { columns: 2 },
                    spacing: 0,
                    alignment,
                },
            )
            .unwrap();
            let horizontal = compose(
                &images,
                &ComposeOptions {
                    layout: Layout::Stack {
                        direction: StackDirection::Horizontal,
                    },
                    spacing: 0,
                    alignment,
                },
            )
            .unwrap();

            assert_canvas(&grid, 6, 4, |x, y| {
                if x >= 4 {
                    BLUE
                } else if (offset..offset + 2).contains(&y) {
                    RED
                } else {
                    BLACK
                }
            });
            assert_same_image(&grid, &horizontal);
        }
    }

    #[test]
    fn one_column_grid_matches_vertical_stack_for_every_alignment() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE)];
        for alignment in [Alignment::Start, Alignment::Center, Alignment::End] {
            let offset = alignment_offset(alignment, 2);
            let grid = compose(
                &images,
                &ComposeOptions {
                    layout: Layout::Grid { columns: 1 },
                    spacing: 0,
                    alignment,
                },
            )
            .unwrap();
            let vertical = compose(
                &images,
                &ComposeOptions {
                    layout: Layout::Stack {
                        direction: StackDirection::Vertical,
                    },
                    spacing: 0,
                    alignment,
                },
            )
            .unwrap();

            assert_canvas(&grid, 4, 6, |x, y| {
                if y < 2 {
                    RED
                } else if (offset..offset + 2).contains(&x) {
                    BLUE
                } else {
                    BLACK
                }
            });
            assert_same_image(&grid, &vertical);
        }
    }

    #[test]
    fn freeform_canvas_fits_non_overlapping_images_at_far_positions() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE)];
        let result = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Freeform {
                    positions: vec![(0, 0), (10, 10)],
                },
                ..ComposeOptions::default()
            },
        )
        .unwrap();

        assert_canvas(&result, 12, 14, |x, y| {
            if x < 4 && y < 2 {
                RED
            } else if (10..12).contains(&x) && (10..14).contains(&y) {
                BLUE
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn freeform_canvas_uses_maximum_extent_per_axis() {
        let images = [solid(10, 1, RED), solid(1, 10, BLUE)];
        let result = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Freeform {
                    positions: vec![(0, 0), (0, 0)],
                },
                ..ComposeOptions::default()
            },
        )
        .unwrap();

        assert_canvas(&result, 10, 10, |x, y| {
            if x == 0 {
                BLUE
            } else if y == 0 {
                RED
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn freeform_partial_overlap_is_painted_in_input_order() {
        let images = [solid(4, 4, RED), solid(4, 4, BLUE)];
        let result = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Freeform {
                    positions: vec![(0, 0), (2, 2)],
                },
                ..ComposeOptions::default()
            },
        )
        .unwrap();

        assert_canvas(&result, 6, 6, |x, y| {
            if x >= 2 && y >= 2 {
                BLUE
            } else if x < 4 && y < 4 {
                RED
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn freeform_preserves_leading_empty_canvas_margin() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE)];
        let result = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Freeform {
                    positions: vec![(5, 5), (5, 5)],
                },
                ..ComposeOptions::default()
            },
        )
        .unwrap();

        assert_canvas(&result, 9, 9, |x, y| {
            if (5..7).contains(&x) && (5..9).contains(&y) {
                BLUE
            } else if (5..9).contains(&x) && (5..7).contains(&y) {
                RED
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn semi_transparent_image_blends_onto_background_and_stays_opaque() {
        let images = [solid(2, 2, RED), solid(1, 2, [0, 0, 255, 128])];
        let result = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Stack {
                    direction: StackDirection::Horizontal,
                },
                spacing: 1,
                alignment: Alignment::Start,
            },
        )
        .unwrap();

        assert_canvas(&result, 6, 4, |x, y| {
            if (1..3).contains(&x) && (1..3).contains(&y) {
                RED
            } else if x == 4 && (1..3).contains(&y) {
                [0, 0, 128, 255]
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn custom_background_fills_spacing_and_alignment_slack() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE)];
        let spec = spec_for(
            &images,
            SpecLayout::Stack {
                direction: StackDirection::Horizontal,
                spacing: 1,
                alignment: Alignment::Start,
            },
            RgbColor {
                red: PURPLE[0],
                green: PURPLE[1],
                blue: PURPLE[2],
            },
        );
        let result = render_spec(&spec, &images).unwrap();

        assert_canvas(&result, 9, 6, |x, y| {
            if (1..5).contains(&x) && (1..3).contains(&y) {
                RED
            } else if (6..8).contains(&x) && (1..5).contains(&y) {
                BLUE
            } else {
                PURPLE
            }
        });
    }

    #[test]
    fn jpeg_export_keeps_dimensions_and_approximately_preserves_background() {
        let images = [solid(16, 8, RED), solid(8, 16, BLUE)];
        let spec = spec_for(
            &images,
            SpecLayout::Stack {
                direction: StackDirection::Horizontal,
                spacing: 4,
                alignment: Alignment::Start,
            },
            RgbColor {
                red: PURPLE[0],
                green: PURPLE[1],
                blue: PURPLE[2],
            },
        );
        let composed = render_spec(&spec, &images).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("result.jpg");

        assert_eq!(composed.dimensions(), (36, 24));
        export_image(&composed, &output, ExportFormat::Jpeg).unwrap();

        let bytes = std::fs::read(&output).unwrap();
        assert!(bytes.starts_with(&[0xff, 0xd8, 0xff]));
        let decoded = load_image(&output).unwrap();
        assert_eq!(decoded.dimensions(), (36, 24));
        let background_pixel = decoded.get_pixel(12, 16).0;
        for channel in 0..3 {
            assert!(
                background_pixel[channel].abs_diff(PURPLE[channel]) <= 20,
                "unexpected JPEG background channel: {}",
                background_pixel[channel]
            );
        }
        assert_eq!(background_pixel[3], 255);
    }

    #[test]
    fn three_single_pixel_images_cover_layouts_and_leave_ragged_grid_cell_empty() {
        let images = [solid(1, 1, RED), solid(1, 1, BLUE), solid(1, 1, GREEN)];

        let horizontal = compose_with(
            &images,
            Layout::Stack {
                direction: StackDirection::Horizontal,
            },
            0,
            Alignment::Start,
        );
        assert_canvas(&horizontal, 3, 1, |x, _| match x {
            0 => RED,
            1 => BLUE,
            _ => GREEN,
        });

        let vertical = compose_with(
            &images,
            Layout::Stack {
                direction: StackDirection::Vertical,
            },
            0,
            Alignment::Start,
        );
        assert_canvas(&vertical, 1, 3, |_, y| match y {
            0 => RED,
            1 => BLUE,
            _ => GREEN,
        });

        let grid_two_columns =
            compose_with(&images, Layout::Grid { columns: 2 }, 0, Alignment::Start);
        assert_canvas(&grid_two_columns, 2, 2, |x, y| match (x, y) {
            (0, 0) => RED,
            (1, 0) => BLUE,
            (0, 1) => GREEN,
            _ => BLACK,
        });

        let grid_one_column =
            compose_with(&images, Layout::Grid { columns: 1 }, 0, Alignment::Start);
        assert_same_image(&vertical, &grid_one_column);

        let grid_three_columns =
            compose_with(&images, Layout::Grid { columns: 3 }, 0, Alignment::Start);
        let grid_many_columns =
            compose_with(&images, Layout::Grid { columns: 99 }, 0, Alignment::Start);
        assert_same_image(&horizontal, &grid_three_columns);
        assert_same_image(&horizontal, &grid_many_columns);

        let freeform = compose_with(
            &images,
            Layout::Freeform {
                positions: vec![(0, 0), (0, 0), (0, 0)],
            },
            0,
            Alignment::Start,
        );
        assert_canvas(&freeform, 1, 1, |_, _| GREEN);
    }

    #[test]
    fn repeated_path_three_times_is_rendered_three_times() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("same.png");
        export_image(&solid(2, 1, RED), &input, ExportFormat::Png).unwrap();
        let loaded = [
            load_image_with_metadata(&input).unwrap(),
            load_image_with_metadata(&input).unwrap(),
            load_image_with_metadata(&input).unwrap(),
        ];
        let metadata = loaded
            .iter()
            .map(|image| image.metadata.clone())
            .collect::<Vec<_>>();
        let spec = compose_spec(
            &metadata,
            &CompositionParams {
                layout: SpecLayout::Stack {
                    direction: StackDirection::Horizontal,
                    spacing: 0,
                    alignment: Alignment::Start,
                },
                background_color: RgbColor::default(),
            },
        )
        .unwrap();
        let images = loaded
            .into_iter()
            .map(|image| image.image)
            .collect::<Vec<_>>();
        let result = render_spec(&spec, &images).unwrap();

        assert_canvas(&result, 6, 1, |_, _| RED);
    }

    #[test]
    fn three_single_pixel_images_with_spacing_have_two_single_spacing_gaps() {
        let images = [solid(1, 1, RED), solid(1, 1, BLUE), solid(1, 1, GREEN)];
        let result = compose_with(
            &images,
            Layout::Stack {
                direction: StackDirection::Horizontal,
            },
            20,
            Alignment::Start,
        );

        assert_canvas(&result, 83, 41, |x, y| match (x, y) {
            (20, 20) => RED,
            (41, 20) => BLUE,
            (62, 20) => GREEN,
            _ => BLACK,
        });
    }

    #[test]
    fn freeform_rejects_two_or_four_positions_for_three_images() {
        let metadata = [
            ImageMetadata {
                name: "a.png".into(),
                width: 1,
                height: 1,
            },
            ImageMetadata {
                name: "b.png".into(),
                width: 1,
                height: 1,
            },
            ImageMetadata {
                name: "c.png".into(),
                width: 1,
                height: 1,
            },
        ];
        for (positions, actual) in [
            (vec![(0, 0), (1, 1)], 2),
            (vec![(0, 0), (1, 1), (2, 2), (3, 3)], 4),
        ] {
            assert_eq!(
                compose_spec(
                    &metadata,
                    &CompositionParams {
                        layout: SpecLayout::Freeform {
                            positions: positions
                                .iter()
                                .map(|&(x, y)| SpecPosition { x, y })
                                .collect(),
                        },
                        background_color: RgbColor::default(),
                    }
                )
                .err(),
                Some(ComposeError::FreeformPositionCount {
                    expected: 3,
                    actual,
                })
            );
        }
    }

    #[test]
    fn invalid_image_in_the_middle_is_not_hidden_by_valid_images() {
        let images = [
            solid(1, 1, RED),
            DynamicImage::ImageRgba8(RgbaImage::new(0, 3)),
            solid(1, 1, BLUE),
        ];

        assert_eq!(
            compose(&images, &ComposeOptions::default()).err(),
            Some(ComposeError::InvalidImageDimensions)
        );
    }

    #[test]
    fn three_image_axis_accumulation_overflow_is_rejected() {
        let half = u32::MAX / 2;
        let wide_images = [
            ImageMetadata {
                name: "a.png".into(),
                width: half,
                height: 1,
            },
            ImageMetadata {
                name: "b.png".into(),
                width: half,
                height: 1,
            },
            ImageMetadata {
                name: "c.png".into(),
                width: half,
                height: 1,
            },
        ];
        let tall_images = [
            ImageMetadata {
                name: "a.png".into(),
                width: 1,
                height: half,
            },
            ImageMetadata {
                name: "b.png".into(),
                width: 1,
                height: half,
            },
            ImageMetadata {
                name: "c.png".into(),
                width: 1,
                height: half,
            },
        ];

        assert_eq!(
            compose_spec(
                &wide_images,
                &CompositionParams {
                    layout: SpecLayout::Stack {
                        direction: StackDirection::Horizontal,
                        spacing: 0,
                        alignment: Alignment::Start,
                    },
                    background_color: RgbColor::default(),
                }
            )
            .err(),
            Some(ComposeError::SizeOverflow)
        );
        assert_eq!(
            compose_spec(
                &tall_images,
                &CompositionParams {
                    layout: SpecLayout::Grid {
                        columns: 1,
                        spacing: 0,
                        alignment: Alignment::Start,
                    },
                    background_color: RgbColor::default(),
                }
            )
            .err(),
            Some(ComposeError::SizeOverflow)
        );
    }

    #[test]
    fn horizontal_stack_aligns_each_of_three_images_independently() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE), solid(6, 1, GREEN)];
        for alignment in [Alignment::Start, Alignment::Center, Alignment::End] {
            let a_offset = alignment_offset(alignment, 2);
            let c_offset = alignment_offset(alignment, 3);
            let result = compose_with(
                &images,
                Layout::Stack {
                    direction: StackDirection::Horizontal,
                },
                0,
                alignment,
            );

            assert_canvas(&result, 12, 4, |x, y| {
                if x < 4 && (a_offset..a_offset + 2).contains(&y) {
                    RED
                } else if (4..6).contains(&x) {
                    BLUE
                } else if x >= 6 && y == c_offset {
                    GREEN
                } else {
                    BLACK
                }
            });
        }
    }

    #[test]
    fn vertical_stack_aligns_each_of_three_images_independently() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE), solid(6, 1, GREEN)];
        for alignment in [Alignment::Start, Alignment::Center, Alignment::End] {
            let a_offset = alignment_offset(alignment, 2);
            let b_offset = alignment_offset(alignment, 4);
            let result = compose_with(
                &images,
                Layout::Stack {
                    direction: StackDirection::Vertical,
                },
                0,
                alignment,
            );

            assert_canvas(&result, 6, 7, |x, y| {
                if y < 2 && (a_offset..a_offset + 4).contains(&x) {
                    RED
                } else if (2..6).contains(&y) && (b_offset..b_offset + 2).contains(&x) {
                    BLUE
                } else if y == 6 {
                    GREEN
                } else {
                    BLACK
                }
            });
        }
    }

    #[test]
    fn horizontal_stack_keeps_cross_axis_maximum_in_the_middle() {
        let images = [solid(2, 4, BLUE), solid(4, 2, RED), solid(6, 1, GREEN)];
        for alignment in [Alignment::Start, Alignment::Center, Alignment::End] {
            let a_offset = alignment_offset(alignment, 2);
            let c_offset = alignment_offset(alignment, 3);
            let result = compose_with(
                &images,
                Layout::Stack {
                    direction: StackDirection::Horizontal,
                },
                0,
                alignment,
            );

            assert_canvas(&result, 12, 4, |x, y| {
                if x < 2 {
                    BLUE
                } else if (2..6).contains(&x) && (a_offset..a_offset + 2).contains(&y) {
                    RED
                } else if x >= 6 && y == c_offset {
                    GREEN
                } else {
                    BLACK
                }
            });
        }
    }

    #[test]
    fn horizontal_stack_keeps_cross_axis_maximum_last() {
        let images = [solid(4, 2, RED), solid(6, 1, GREEN), solid(2, 4, BLUE)];
        for alignment in [Alignment::Start, Alignment::Center, Alignment::End] {
            let a_offset = alignment_offset(alignment, 2);
            let c_offset = alignment_offset(alignment, 3);
            let result = compose_with(
                &images,
                Layout::Stack {
                    direction: StackDirection::Horizontal,
                },
                0,
                alignment,
            );

            assert_canvas(&result, 12, 4, |x, y| {
                if x < 4 && (a_offset..a_offset + 2).contains(&y) {
                    RED
                } else if (4..10).contains(&x) && y == c_offset {
                    GREEN
                } else if x >= 10 {
                    BLUE
                } else {
                    BLACK
                }
            });
        }
    }

    #[test]
    fn spacing_with_three_images_preserves_margins_gaps_and_alignment_offsets() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE), solid(6, 1, GREEN)];
        let result = compose_with(
            &images,
            Layout::Stack {
                direction: StackDirection::Horizontal,
            },
            3,
            Alignment::Center,
        );

        assert_canvas(&result, 24, 10, |x, y| {
            if (3..7).contains(&x) && (4..6).contains(&y) {
                RED
            } else if (10..12).contains(&x) && (3..7).contains(&y) {
                BLUE
            } else if (15..21).contains(&x) && y == 4 {
                GREEN
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn ragged_two_column_grid_uses_column_and_row_maxima() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE), solid(6, 1, GREEN)];
        let result = compose_with(&images, Layout::Grid { columns: 2 }, 0, Alignment::Start);

        assert_canvas(&result, 8, 5, |x, y| {
            if x < 4 && y < 2 {
                RED
            } else if (6..8).contains(&x) && y < 4 {
                BLUE
            } else if x < 6 && y == 4 {
                GREEN
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn ragged_grid_alignment_shifts_within_each_cell() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE), solid(6, 1, GREEN)];
        for alignment in [Alignment::Start, Alignment::Center, Alignment::End] {
            let offset = alignment_offset(alignment, 2);
            let result = compose_with(&images, Layout::Grid { columns: 2 }, 0, alignment);

            assert_canvas(&result, 8, 5, |x, y| {
                if (offset..offset + 4).contains(&x) && (offset..offset + 2).contains(&y) {
                    RED
                } else if (6..8).contains(&x) && y < 4 {
                    BLUE
                } else if x < 6 && y == 4 {
                    GREEN
                } else {
                    BLACK
                }
            });
        }
    }

    #[test]
    fn ragged_grid_spacing_fills_inter_cell_gaps_and_empty_cells() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE), solid(6, 1, GREEN)];
        let result = compose_with(&images, Layout::Grid { columns: 2 }, 2, Alignment::Start);

        assert_canvas(&result, 14, 11, |x, y| {
            if (2..6).contains(&x) && (2..4).contains(&y) {
                RED
            } else if (10..12).contains(&x) && (2..6).contains(&y) {
                BLUE
            } else if (2..8).contains(&x) && y == 8 {
                GREEN
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn five_image_two_column_grid_uses_each_row_height() {
        let images = [
            solid(2, 1, RED),
            solid(1, 2, BLUE),
            solid(3, 1, GREEN),
            solid(1, 4, YELLOW),
            solid(2, 2, MAGENTA),
        ];
        let result = compose_with(&images, Layout::Grid { columns: 2 }, 0, Alignment::Start);

        assert_canvas(&result, 4, 8, |x, y| {
            if x < 2 && y == 0 {
                RED
            } else if x == 3 && y < 2 {
                BLUE
            } else if x < 3 && y == 2 {
                GREEN
            } else if x == 3 && (2..6).contains(&y) {
                YELLOW
            } else if x < 2 && (6..8).contains(&y) {
                MAGENTA
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn five_image_three_column_grid_leaves_last_column_empty_in_final_row() {
        let images = [
            solid(2, 2, RED),
            solid(3, 1, BLUE),
            solid(5, 1, GREEN),
            solid(4, 2, YELLOW),
            solid(1, 3, MAGENTA),
        ];
        let result = compose_with(&images, Layout::Grid { columns: 3 }, 0, Alignment::Start);

        assert_canvas(&result, 12, 5, |x, y| {
            if x < 2 && y < 2 {
                RED
            } else if (4..7).contains(&x) && y == 0 {
                BLUE
            } else if (7..12).contains(&x) && y == 0 {
                GREEN
            } else if x < 4 && (2..4).contains(&y) {
                YELLOW
            } else if x == 4 && (2..5).contains(&y) {
                MAGENTA
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn four_image_three_column_grid_leaves_two_final_cells_empty() {
        let images = [
            solid(1, 1, RED),
            solid(2, 1, BLUE),
            solid(3, 1, GREEN),
            solid(4, 2, YELLOW),
        ];
        let result = compose_with(&images, Layout::Grid { columns: 3 }, 0, Alignment::Start);

        assert_canvas(&result, 9, 3, |x, y| {
            if x == 0 && y == 0 {
                RED
            } else if (4..6).contains(&x) && y == 0 {
                BLUE
            } else if (6..9).contains(&x) && y == 0 {
                GREEN
            } else if x < 4 && (1..3).contains(&y) {
                YELLOW
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn grids_with_at_least_three_columns_match_horizontal_stack() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE), solid(6, 1, GREEN)];
        for alignment in [Alignment::Start, Alignment::Center, Alignment::End] {
            let horizontal = compose_with(
                &images,
                Layout::Stack {
                    direction: StackDirection::Horizontal,
                },
                0,
                alignment,
            );
            for columns in [3, 4, 100] {
                let grid = compose_with(&images, Layout::Grid { columns }, 0, alignment);
                assert_same_image(&horizontal, &grid);
            }
        }
    }

    #[test]
    fn one_column_grid_matches_vertical_stack_for_three_images() {
        let images = [solid(4, 2, RED), solid(2, 4, BLUE), solid(6, 1, GREEN)];
        for alignment in [Alignment::Start, Alignment::Center, Alignment::End] {
            let vertical = compose_with(
                &images,
                Layout::Stack {
                    direction: StackDirection::Vertical,
                },
                0,
                alignment,
            );
            let grid = compose_with(&images, Layout::Grid { columns: 1 }, 0, alignment);

            assert_same_image(&vertical, &grid);
        }
    }

    #[test]
    fn grid_dimensions_change_with_image_order_and_column_maxima() {
        let base = [solid(4, 2, RED), solid(2, 4, BLUE), solid(6, 1, GREEN)];
        let cases = [
            ([0, 1, 2], (8, 5)),
            ([0, 2, 1], (10, 6)),
            ([1, 0, 2], (10, 5)),
            ([1, 2, 0], (10, 6)),
            ([2, 0, 1], (10, 6)),
            ([2, 1, 0], (8, 6)),
        ];

        for (order, expected_dimensions) in cases {
            let images = order.map(|index| base[index].clone());
            let result = compose_with(&images, Layout::Grid { columns: 2 }, 0, Alignment::Start);
            assert_eq!(result.dimensions(), expected_dimensions);
        }
    }

    #[test]
    fn full_two_by_two_grid_places_four_different_images_and_background_slack() {
        let images = [
            solid(4, 2, RED),
            solid(2, 4, BLUE),
            solid(6, 1, GREEN),
            solid(3, 3, YELLOW),
        ];
        let result = compose_with(&images, Layout::Grid { columns: 2 }, 0, Alignment::Start);

        assert_canvas(&result, 9, 7, |x, y| {
            if x < 4 && y < 2 {
                RED
            } else if (6..8).contains(&x) && y < 4 {
                BLUE
            } else if x < 6 && y == 4 {
                GREEN
            } else if (6..9).contains(&x) && (4..7).contains(&y) {
                YELLOW
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn center_alignment_floors_odd_slack_on_both_grid_axes() {
        let images = [
            solid(3, 3, RED),
            solid(2, 4, BLUE),
            solid(4, 2, GREEN),
            solid(1, 1, YELLOW),
        ];
        let result = compose_with(&images, Layout::Grid { columns: 2 }, 0, Alignment::Center);

        assert_canvas(&result, 6, 6, |x, y| {
            if x < 3 && y < 3 {
                RED
            } else if (4..6).contains(&x) && y < 4 {
                BLUE
            } else if x < 4 && (4..6).contains(&y) {
                GREEN
            } else if x == 4 && y == 4 {
                YELLOW
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn six_image_three_column_grid_uses_per_column_and_row_maxima() {
        let images = [
            solid(2, 1, RED),
            solid(3, 2, BLUE),
            solid(1, 4, GREEN),
            solid(4, 3, YELLOW),
            solid(1, 1, MAGENTA),
            solid(2, 2, CYAN),
        ];
        let result = compose_with(&images, Layout::Grid { columns: 3 }, 0, Alignment::Start);

        assert_canvas(&result, 9, 7, |x, y| {
            if x < 2 && y == 0 {
                RED
            } else if (4..7).contains(&x) && (0..2).contains(&y) {
                BLUE
            } else if (7..8).contains(&x) && y < 4 {
                GREEN
            } else if x < 4 && (4..7).contains(&y) {
                YELLOW
            } else if x == 4 && y == 4 {
                MAGENTA
            } else if (7..9).contains(&x) && (4..6).contains(&y) {
                CYAN
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn four_image_grid_order_changes_row_and_column_maxima() {
        let base = [
            solid(4, 2, RED),
            solid(2, 4, BLUE),
            solid(6, 1, GREEN),
            solid(1, 6, YELLOW),
        ];
        let cases = [([0, 1, 2, 3], (8, 10)), ([0, 2, 1, 3], (10, 8))];

        for (order, expected_dimensions) in cases {
            let images = order.map(|index| base[index].clone());
            let result = compose_with(&images, Layout::Grid { columns: 2 }, 0, Alignment::Start);
            assert_eq!(result.dimensions(), expected_dimensions);
        }
    }

    #[test]
    fn three_image_freeform_chain_overlap_uses_last_image() {
        let images = [solid(4, 4, RED), solid(4, 4, BLUE), solid(4, 4, GREEN)];
        let result = compose_with(
            &images,
            Layout::Freeform {
                positions: vec![(0, 0), (2, 2), (4, 4)],
            },
            0,
            Alignment::Start,
        );

        assert_canvas(&result, 8, 8, |x, y| {
            if x >= 4 && y >= 4 {
                GREEN
            } else if (2..6).contains(&x) && (2..6).contains(&y) {
                BLUE
            } else if x < 4 && y < 4 {
                RED
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn freeform_occludes_middle_image_and_reordering_changes_visible_owner() {
        let red = solid(4, 4, RED);
        let blue = solid(2, 2, BLUE);
        let green = solid(2, 2, GREEN);
        let positions = vec![(0, 0), (2, 2), (2, 2)];
        let forward = compose_with(
            &[red.clone(), blue.clone(), green.clone()],
            Layout::Freeform {
                positions: positions.clone(),
            },
            0,
            Alignment::Start,
        );
        assert_canvas(
            &forward,
            4,
            4,
            |x, y| {
                if x >= 2 && y >= 2 {
                    GREEN
                } else {
                    RED
                }
            },
        );

        let blue_last = compose_with(
            &[red, green, blue],
            Layout::Freeform { positions },
            0,
            Alignment::Start,
        );
        assert_canvas(
            &blue_last,
            4,
            4,
            |x, y| {
                if x >= 2 && y >= 2 {
                    BLUE
                } else {
                    RED
                }
            },
        );
    }

    #[test]
    fn freeform_canvas_extent_comes_from_different_images_on_each_axis() {
        let images = [solid(10, 1, RED), solid(1, 10, BLUE), solid(2, 2, GREEN)];
        let result = compose_with(
            &images,
            Layout::Freeform {
                positions: vec![(5, 0), (0, 7), (3, 3)],
            },
            0,
            Alignment::Start,
        );

        assert_canvas(&result, 15, 17, |x, y| {
            if (5..15).contains(&x) && y == 0 {
                RED
            } else if x == 0 && (7..17).contains(&y) {
                BLUE
            } else if (3..5).contains(&x) && (3..5).contains(&y) {
                GREEN
            } else {
                BLACK
            }
        });
    }

    #[test]
    fn three_different_sized_images_at_origin_keep_input_paint_order() {
        let images = [solid(4, 4, RED), solid(2, 2, BLUE), solid(3, 3, GREEN)];
        let result = compose_with(
            &images,
            Layout::Freeform {
                positions: vec![(0, 0), (0, 0), (0, 0)],
            },
            0,
            Alignment::Start,
        );

        assert_canvas(
            &result,
            4,
            4,
            |x, y| {
                if x < 3 && y < 3 {
                    GREEN
                } else {
                    RED
                }
            },
        );
    }

    #[test]
    fn sparse_three_image_freeform_canvas_has_background_between_images() {
        let images = [solid(2, 2, RED), solid(3, 2, BLUE), solid(1, 4, GREEN)];
        let result = compose_with(
            &images,
            Layout::Freeform {
                positions: vec![(0, 0), (20, 10), (40, 30)],
            },
            0,
            Alignment::Start,
        );

        assert_canvas(&result, 41, 34, |x, y| {
            if x < 2 && y < 2 {
                RED
            } else if (20..23).contains(&x) && (10..12).contains(&y) {
                BLUE
            } else if x == 40 && (30..34).contains(&y) {
                GREEN
            } else {
                BLACK
            }
        });
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

    fn split_spec(
        images: &[DynamicImage],
        divider: u32,
        background_color: RgbColor,
    ) -> CompositionSpec {
        spec_for(images, SpecLayout::Split { divider }, background_color)
    }

    fn count_color(image: &DynamicImage, color: [u8; 4]) -> usize {
        image
            .to_rgba8()
            .pixels()
            .filter(|pixel| pixel.0 == color)
            .count()
    }

    #[test]
    fn split_overlays_same_size_images_along_a_slanted_diagonal() {
        let images = [solid(10, 5, RED), solid(10, 5, BLUE)];
        let spec = split_spec(&images, 0, RgbColor::default());

        assert_eq!((spec.width, spec.height), (10, 5));
        let result = render_spec(&spec, &images).unwrap();
        let first_blue_column_per_row = [6, 5, 5, 5, 4];
        assert_canvas(&result, 10, 5, |x, y| {
            if x < first_blue_column_per_row[y as usize] {
                RED
            } else {
                BLUE
            }
        });
    }

    #[test]
    fn split_divides_many_images_into_equal_bands_in_input_order() {
        let images = [
            solid(30, 10, RED),
            solid(30, 10, GREEN),
            solid(30, 10, BLUE),
        ];
        let result = render_spec(&split_spec(&images, 0, RgbColor::default()), &images).unwrap();

        assert_eq!(result.get_pixel(0, 9).0, RED);
        assert_eq!(result.get_pixel(15, 5).0, GREEN);
        assert_eq!(result.get_pixel(29, 0).0, BLUE);
        for color in [RED, GREEN, BLUE] {
            assert!(count_color(&result, color).abs_diff(100) <= 5);
        }
    }

    #[test]
    fn split_divider_is_painted_with_the_background_color() {
        let images = [
            solid(40, 20, RED),
            solid(40, 20, GREEN),
            solid(40, 20, BLUE),
        ];
        let background = RgbColor {
            red: 102,
            green: 51,
            blue: 153,
        };
        let without = render_spec(&split_spec(&images, 0, background), &images).unwrap();
        let with = render_spec(&split_spec(&images, 4, background), &images).unwrap();

        assert_eq!(count_color(&without, PURPLE), 0);
        assert!(count_color(&with, PURPLE) > 2 * 20 * 3);
        assert_eq!(with.get_pixel(0, 19).0, RED);
        assert_eq!(with.get_pixel(20, 10).0, GREEN);
        assert_eq!(with.get_pixel(39, 0).0, BLUE);
        assert!((0..40).any(|x| with.get_pixel(x, 10).0 == PURPLE && x < 20));
        assert!((0..40).any(|x| with.get_pixel(x, 10).0 == PURPLE && x > 20));
    }

    #[test]
    fn split_blends_transparent_pixels_onto_the_background() {
        let images = [solid(2, 2, [255, 0, 0, 0]), solid(2, 2, [255, 0, 0, 0])];
        let background = RgbColor {
            red: 0,
            green: 255,
            blue: 0,
        };
        let result = render_spec(&split_spec(&images, 0, background), &images).unwrap();

        assert_canvas(&result, 2, 2, |_, _| GREEN);
    }

    #[test]
    fn split_of_a_single_image_reproduces_it() {
        let images = [solid(3, 2, CYAN)];
        let result = render_spec(&split_spec(&images, 10, RgbColor::default()), &images).unwrap();

        assert_canvas(&result, 3, 2, |_, _| CYAN);
    }

    #[test]
    fn split_rejects_images_of_different_sizes() {
        let images = [solid(2, 2, RED), solid(2, 3, BLUE)];
        let metadata = images
            .iter()
            .map(|image| ImageMetadata {
                name: "image".into(),
                width: image.width(),
                height: image.height(),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            compose_spec(
                &metadata,
                &CompositionParams {
                    layout: SpecLayout::Split { divider: 0 },
                    background_color: RgbColor::default(),
                },
            )
            .err(),
            Some(ComposeError::SplitSizeMismatch)
        );
        assert_eq!(
            compose(
                &images,
                &ComposeOptions {
                    layout: Layout::Split { divider: 0 },
                    ..ComposeOptions::default()
                },
            )
            .err(),
            Some(ComposeError::SplitSizeMismatch)
        );

        let spec = split_spec(
            &[solid(2, 2, RED), solid(2, 2, BLUE)],
            0,
            RgbColor::default(),
        );
        assert_eq!(
            render_spec(&spec, &images).err(),
            Some(ComposeError::SplitSizeMismatch)
        );
    }

    #[test]
    fn legacy_compose_matches_spec_rendering_for_split() {
        let images = [
            solid(12, 6, RED),
            solid(12, 6, YELLOW),
            solid(12, 6, MAGENTA),
        ];
        let composed = compose(
            &images,
            &ComposeOptions {
                layout: Layout::Split { divider: 2 },
                spacing: 7,
                alignment: Alignment::End,
            },
        )
        .unwrap();
        let rendered = render_spec(&split_spec(&images, 2, RgbColor::default()), &images).unwrap();

        assert_same_image(&composed, &rendered);
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
