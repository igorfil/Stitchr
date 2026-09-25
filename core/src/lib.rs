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

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
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

    fn alignment_offset(alignment: Alignment, slack: u32) -> u32 {
        match alignment {
            Alignment::Start => 0,
            Alignment::Center => slack / 2,
            Alignment::End => slack,
        }
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
    fn repeated_input_path_is_rendered_twice() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("a.png");
        export_image(&solid(2, 1, RED), &input, ExportFormat::Png).unwrap();
        let path = input.to_str().unwrap().to_owned();
        let spec = CompositionSpec {
            paths: vec![path.clone(), path],
            layout: SpecLayout::Stack {
                direction: StackDirection::Horizontal,
                spacing: 0,
                alignment: Alignment::Start,
            },
            background_color: RgbColor::default(),
        };

        let result = render_paths(&spec).unwrap();

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

        let spec = CompositionSpec {
            paths: vec!["wide.png".into(), "small.png".into()],
            layout: SpecLayout::Grid {
                columns: 2,
                spacing: 0,
                alignment: Alignment::Start,
            },
            background_color: RgbColor::default(),
        };
        assert_eq!(
            plan_spec(&spec, &[(u32::MAX, 1), (1, 1)]),
            Err(ComposeError::SizeOverflow)
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
        let spec = CompositionSpec {
            paths: vec!["a.png".into(), "b.png".into()],
            layout: SpecLayout::Stack {
                direction: StackDirection::Horizontal,
                spacing: 1,
                alignment: Alignment::Start,
            },
            background_color: RgbColor {
                red: PURPLE[0],
                green: PURPLE[1],
                blue: PURPLE[2],
            },
        };
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
        let spec = CompositionSpec {
            paths: vec!["a.png".into(), "b.png".into()],
            layout: SpecLayout::Stack {
                direction: StackDirection::Horizontal,
                spacing: 4,
                alignment: Alignment::Start,
            },
            background_color: RgbColor {
                red: PURPLE[0],
                green: PURPLE[1],
                blue: PURPLE[2],
            },
        };
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
