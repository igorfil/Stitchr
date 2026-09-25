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
}
