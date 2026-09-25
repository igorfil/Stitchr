use crate::{CliAlignment, CliLayout};
use stitchr_core::{
    compose_spec, Alignment, CompositionParams, CompositionSpec, ImageMetadata, RgbColor,
    SpecLayout, SpecPosition, StackDirection,
};

pub(crate) struct CliComposer;

pub(crate) struct CliComposeParams<'a> {
    pub layout: CliLayout,
    pub columns: u32,
    pub spacing: u32,
    pub alignment: CliAlignment,
    pub background_color: RgbColor,
    pub positions: &'a [u32],
}

impl CliComposer {
    pub(crate) fn compose(
        images: &[ImageMetadata],
        cli_params: CliComposeParams<'_>,
    ) -> Result<CompositionSpec, String> {
        let alignment = match cli_params.alignment {
            CliAlignment::Start => Alignment::Start,
            CliAlignment::Center => Alignment::Center,
            CliAlignment::End => Alignment::End,
        };
        let layout = match cli_params.layout {
            CliLayout::Grid => SpecLayout::Grid {
                columns: cli_params.columns,
                spacing: cli_params.spacing,
                alignment,
            },
            CliLayout::Horizontal => SpecLayout::Stack {
                direction: StackDirection::Horizontal,
                spacing: cli_params.spacing,
                alignment,
            },
            CliLayout::Vertical => SpecLayout::Stack {
                direction: StackDirection::Vertical,
                spacing: cli_params.spacing,
                alignment,
            },
            CliLayout::Freeform => SpecLayout::Freeform {
                positions: cli_params
                    .positions
                    .chunks_exact(2)
                    .map(|pair| SpecPosition {
                        x: pair[0],
                        y: pair[1],
                    })
                    .collect(),
            },
        };
        compose_spec(
            images,
            &CompositionParams {
                layout,
                background_color: cli_params.background_color,
            },
        )
        .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use clap::Parser;
    use stitchr_core::ImagePlacement;

    fn cli_params(cli: &Cli) -> CliComposeParams<'_> {
        CliComposeParams {
            layout: cli.layout,
            columns: cli.columns,
            spacing: cli.spacing,
            alignment: cli.alignment,
            background_color: cli.background_color,
            positions: &cli.position,
        }
    }

    fn compose_cli(cli: &Cli) -> CompositionSpec {
        let images = cli
            .images
            .iter()
            .map(|path| ImageMetadata {
                name: path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                width: 3,
                height: 2,
            })
            .collect::<Vec<_>>();
        CliComposer::compose(&images, cli_params(cli)).unwrap()
    }

    fn compose_args(args: &[&str]) -> CompositionSpec {
        let cli =
            Cli::try_parse_from(std::iter::once("stitchr").chain(args.iter().copied())).unwrap();
        compose_cli(&cli)
    }

    #[test]
    fn defaults_and_explicit_zero_spacing_build_the_same_spec() {
        let default = compose_args(&["image.png"]);
        let explicit_zero = compose_args(&["image.png", "--spacing", "0"]);

        assert_eq!(
            default,
            CompositionSpec {
                images: vec![ImagePlacement {
                    name: "image.png".into(),
                    position: SpecPosition { x: 0, y: 0 },
                }],
                layout: SpecLayout::Grid {
                    columns: 2,
                    spacing: 0,
                    alignment: Alignment::Start,
                },
                width: 3,
                height: 2,
                background_color: RgbColor::default(),
            }
        );
        assert_eq!(explicit_zero, default);
    }

    #[test]
    fn builds_spec_without_opening_input_images() {
        let cli = Cli::try_parse_from([
            "stitchr",
            "missing-one.png",
            "missing-two.jpg",
            "--layout",
            "grid",
            "--spacing",
            "10",
            "--print-spec",
        ])
        .unwrap();
        let spec = compose_cli(&cli);

        assert!(cli.print_spec);
        assert_eq!(
            spec,
            CompositionSpec {
                images: vec![
                    ImagePlacement {
                        name: "missing-one.png".into(),
                        position: SpecPosition { x: 10, y: 10 },
                    },
                    ImagePlacement {
                        name: "missing-two.jpg".into(),
                        position: SpecPosition { x: 23, y: 10 },
                    },
                ],
                layout: SpecLayout::Grid {
                    columns: 2,
                    spacing: 10,
                    alignment: Alignment::Start,
                },
                width: 36,
                height: 22,
                background_color: RgbColor::default(),
            }
        );
    }

    #[test]
    fn grid_spec_preserves_spacing_columns_and_alignment() {
        let spec = compose_args(&[
            "image.png",
            "--spacing",
            "20",
            "--columns",
            "5",
            "--alignment",
            "end",
        ]);

        assert_eq!(
            spec.layout,
            SpecLayout::Grid {
                columns: 5,
                spacing: 20,
                alignment: Alignment::End,
            }
        );
        assert_eq!(spec.background_color, RgbColor::default());
        assert_eq!((spec.width, spec.height), (43, 42));
        assert_eq!(spec.images[0].position, SpecPosition { x: 20, y: 20 });
    }

    #[test]
    fn grid_background_and_spacing_are_preserved_for_equivalent_color_forms() {
        let named = compose_args(&[
            "image.png",
            "--spacing",
            "20",
            "--background",
            "rebeccapurple",
        ]);
        let rgb = compose_args(&[
            "image.png",
            "--spacing",
            "20",
            "--background",
            "rgb(102,51,153)",
        ]);
        let custom_background_without_spacing =
            compose_args(&["image.png", "--background", "rebeccapurple"]);

        assert_eq!(named, rgb);
        assert_eq!(
            named.layout,
            SpecLayout::Grid {
                columns: 2,
                spacing: 20,
                alignment: Alignment::Start,
            }
        );
        assert_eq!(
            custom_background_without_spacing.layout,
            SpecLayout::Grid {
                columns: 2,
                spacing: 0,
                alignment: Alignment::Start,
            }
        );
        assert_eq!(
            named.background_color,
            RgbColor {
                red: 102,
                green: 51,
                blue: 153,
            }
        );
    }

    #[test]
    fn stack_specs_preserve_layout_direction_and_spacing_for_one_image() {
        let horizontal = compose_args(&[
            "image.png",
            "--layout",
            "horizontal",
            "--spacing",
            "20",
            "--alignment",
            "center",
        ]);
        let vertical = compose_args(&[
            "image.png",
            "--layout",
            "vertical",
            "--spacing",
            "20",
            "--alignment",
            "center",
        ]);

        assert_eq!(
            horizontal.layout,
            SpecLayout::Stack {
                direction: StackDirection::Horizontal,
                spacing: 20,
                alignment: Alignment::Center,
            }
        );
        assert_eq!(
            vertical.layout,
            SpecLayout::Stack {
                direction: StackDirection::Vertical,
                spacing: 20,
                alignment: Alignment::Center,
            }
        );
    }

    #[test]
    fn freeform_spec_contains_only_positions() {
        let cli = Cli::try_parse_from([
            "stitchr",
            "a.png",
            "b.png",
            "--layout",
            "freeform",
            "--columns",
            "5",
            "--spacing",
            "40",
            "--alignment",
            "end",
            "--background",
            "rebeccapurple",
            "--position",
            "1",
            "2",
            "--position",
            "3",
            "4",
        ])
        .unwrap();
        let spec = compose_cli(&cli);
        let without_grid_options = compose_args(&[
            "a.png",
            "b.png",
            "--layout",
            "freeform",
            "--background",
            "rebeccapurple",
            "--position",
            "1",
            "2",
            "--position",
            "3",
            "4",
        ]);

        assert_eq!(
            spec.layout,
            SpecLayout::Freeform {
                positions: vec![SpecPosition { x: 1, y: 2 }, SpecPosition { x: 3, y: 4 }]
            }
        );
        assert_eq!(spec, without_grid_options);
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(json["layout"]["type"], "freeform");
        assert!(json["layout"].get("columns").is_none());
        assert!(json["layout"].get("spacing").is_none());
        assert!(json["layout"].get("alignment").is_none());
        assert_eq!(
            spec.background_color,
            RgbColor {
                red: 102,
                green: 51,
                blue: 153
            }
        );
    }

    #[test]
    fn cli_composer_uses_distinct_metadata_for_three_images() {
        let cli = Cli::try_parse_from([
            "stitchr",
            "a.png",
            "b.png",
            "c.png",
            "--layout",
            "grid",
            "--columns",
            "2",
        ])
        .unwrap();
        let images = [
            ImageMetadata {
                name: "a.png".into(),
                width: 4,
                height: 2,
            },
            ImageMetadata {
                name: "b.png".into(),
                width: 2,
                height: 4,
            },
            ImageMetadata {
                name: "c.png".into(),
                width: 6,
                height: 1,
            },
        ];

        let spec = CliComposer::compose(&images, cli_params(&cli)).unwrap();

        assert_eq!((spec.width, spec.height), (8, 5));
        assert_eq!(
            spec.images
                .iter()
                .map(|image| (image.name.as_str(), image.position.x, image.position.y))
                .collect::<Vec<_>>(),
            [("a.png", 0, 0), ("b.png", 6, 0), ("c.png", 0, 4)]
        );
    }

    #[test]
    fn repeated_freeform_positions_preserve_input_order() {
        let cli = Cli::try_parse_from([
            "stitchr",
            "first.png",
            "second.png",
            "third.png",
            "--layout",
            "freeform",
            "--position",
            "7",
            "8",
            "--position",
            "3",
            "4",
            "--position",
            "1",
            "2",
        ])
        .unwrap();
        let images = [
            ImageMetadata {
                name: "first.png".into(),
                width: 1,
                height: 1,
            },
            ImageMetadata {
                name: "second.png".into(),
                width: 1,
                height: 1,
            },
            ImageMetadata {
                name: "third.png".into(),
                width: 1,
                height: 1,
            },
        ];

        let spec = CliComposer::compose(&images, cli_params(&cli)).unwrap();

        assert_eq!(
            spec.images
                .iter()
                .map(|image| (image.name.as_str(), image.position.x, image.position.y))
                .collect::<Vec<_>>(),
            [
                ("first.png", 7, 8),
                ("second.png", 3, 4),
                ("third.png", 1, 2)
            ]
        );
    }

    #[test]
    fn freeform_spec_preserves_origin_and_offset_positions() {
        let origin = compose_args(&["image.png", "--layout", "freeform", "--position", "0", "0"]);
        let offset = compose_args(&[
            "image.png",
            "--layout",
            "freeform",
            "--position",
            "7",
            "11",
            "--spacing",
            "20",
            "--alignment",
            "end",
        ]);

        assert_eq!(
            origin.layout,
            SpecLayout::Freeform {
                positions: vec![SpecPosition { x: 0, y: 0 }]
            }
        );
        assert_eq!(
            offset.layout,
            SpecLayout::Freeform {
                positions: vec![SpecPosition { x: 7, y: 11 }]
            }
        );
    }

    #[test]
    fn output_filename_does_not_change_the_composition_spec() {
        let png = compose_args(&["image.png", "--output", "combined.png"]);
        let jpeg = compose_args(&["image.png", "--output", "combined.jpg"]);

        assert_eq!(png, jpeg);
    }

    #[test]
    fn background_argument_uses_the_shared_color_parser() {
        let named = compose_args(&["missing.png", "--background", "rebeccapurple"]);
        let rgb = compose_args(&["missing.png", "--background", "rgb(102,51,153)"]);

        let named_color = named.background_color;
        let rgb_color = rgb.background_color;
        assert_eq!(named_color, rgb_color);
        let expected = RgbColor {
            red: 102,
            green: 51,
            blue: 153,
        };
        assert_eq!(named_color, expected);
        assert!(
            Cli::try_parse_from(["stitchr", "missing.png", "--background", "notacolor"]).is_err()
        );
    }

    #[test]
    fn rejects_invalid_numeric_arguments() {
        assert!(Cli::try_parse_from(["stitchr", "image.png", "--spacing", "-1"]).is_err());
        assert!(Cli::try_parse_from(["stitchr", "image.png", "--columns", "0"]).is_err());
        assert!(Cli::try_parse_from([
            "stitchr",
            "image.png",
            "--layout",
            "freeform",
            "--position",
            "0",
            "invalid",
        ])
        .is_err());
    }
}
