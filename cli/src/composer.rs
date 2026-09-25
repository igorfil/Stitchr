use crate::{Cli, CliAlignment, CliLayout};
use stitchr_core::{Alignment, CompositionSpec, SpecLayout, SpecPosition, StackDirection};

pub(crate) struct CliComposer;

impl CliComposer {
    pub(crate) fn compose(cli: &Cli) -> Result<CompositionSpec, String> {
        let paths = cli
            .images
            .iter()
            .map(|path| {
                path.to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("input path must be UTF-8: {}", path.display()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let alignment = match cli.alignment {
            CliAlignment::Start => Alignment::Start,
            CliAlignment::Center => Alignment::Center,
            CliAlignment::End => Alignment::End,
        };
        let layout = match cli.layout {
            CliLayout::Grid => SpecLayout::Grid {
                columns: cli.columns,
                spacing: cli.spacing,
                alignment,
            },
            CliLayout::Horizontal => SpecLayout::Stack {
                direction: StackDirection::Horizontal,
                spacing: cli.spacing,
                alignment,
            },
            CliLayout::Vertical => SpecLayout::Stack {
                direction: StackDirection::Vertical,
                spacing: cli.spacing,
                alignment,
            },
            CliLayout::Freeform => SpecLayout::Freeform {
                positions: cli
                    .position
                    .chunks_exact(2)
                    .map(|pair| SpecPosition {
                        x: pair[0],
                        y: pair[1],
                    })
                    .collect(),
            },
        };
        Ok(CompositionSpec {
            paths,
            layout,
            background_color: cli.background_color,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use stitchr_core::RgbColor;

    fn compose_args(args: &[&str]) -> CompositionSpec {
        let cli =
            Cli::try_parse_from(std::iter::once("stitchr").chain(args.iter().copied())).unwrap();
        CliComposer::compose(&cli).unwrap()
    }

    #[test]
    fn defaults_and_explicit_zero_spacing_build_the_same_spec() {
        let default = compose_args(&["image.png"]);
        let explicit_zero = compose_args(&["image.png", "--spacing", "0"]);

        assert_eq!(
            default,
            CompositionSpec {
                paths: vec!["image.png".into()],
                layout: SpecLayout::Grid {
                    columns: 2,
                    spacing: 0,
                    alignment: Alignment::Start,
                },
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
        let spec = CliComposer::compose(&cli).unwrap();

        assert!(cli.print_spec);
        assert_eq!(
            spec,
            CompositionSpec {
                paths: vec!["missing-one.png".into(), "missing-two.jpg".into()],
                layout: SpecLayout::Grid {
                    columns: 2,
                    spacing: 10,
                    alignment: Alignment::Start,
                },
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
        let spec = CliComposer::compose(&cli).unwrap();

        assert_eq!(
            spec.layout,
            SpecLayout::Freeform {
                positions: vec![SpecPosition { x: 1, y: 2 }, SpecPosition { x: 3, y: 4 }]
            }
        );
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
        let named =
            Cli::try_parse_from(["stitchr", "missing.png", "--background", "rebeccapurple"])
                .unwrap();
        let rgb =
            Cli::try_parse_from(["stitchr", "missing.png", "--background", "rgb(102,51,153)"])
                .unwrap();

        let named_color = CliComposer::compose(&named).unwrap().background_color;
        let rgb_color = CliComposer::compose(&rgb).unwrap().background_color;
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
