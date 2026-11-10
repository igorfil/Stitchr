use crate::{io, Cli, CliGradientKind, CliLayout};
use clap::{CommandFactory, ValueEnum};
use serde_json::{json, Value};
use stitchr_core::{ExportFormat, LabelWarning};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Warning {
    pub code: &'static str,
    pub reason: String,
    context: Value,
}

impl Warning {
    pub(crate) fn json(&self) -> Value {
        let mut value = self.context.clone();
        value["code"] = json!(self.code);
        value["reason"] = json!(self.reason);
        value
    }
}

/// When an option has an effect; outside these conditions an explicit value is ignored.
enum Applies {
    Layouts(&'static [CliLayout]),
    Labels,
    Gradient,
    LinearGradient,
    JpegOutput,
    Render,
    FileOutput,
}

const STACKS_AND_GRID: &[CliLayout] =
    &[CliLayout::Grid, CliLayout::Horizontal, CliLayout::Vertical];

const RULES: &[(&str, Applies)] = &[
    ("columns", Applies::Layouts(&[CliLayout::Grid])),
    ("spacing", Applies::Layouts(STACKS_AND_GRID)),
    ("alignment", Applies::Layouts(STACKS_AND_GRID)),
    ("position", Applies::Layouts(&[CliLayout::Freeform])),
    ("divider", Applies::Layouts(&[CliLayout::Split])),
    ("border_width", Applies::Labels),
    ("border_color", Applies::Labels),
    ("label_size", Applies::Labels),
    ("gradient_kind", Applies::Gradient),
    ("gradient_angle", Applies::LinearGradient),
    ("jpeg_quality", Applies::JpegOutput),
    ("output", Applies::Render),
    ("format", Applies::Render),
    ("force", Applies::FileOutput),
    ("no_clobber", Applies::FileOutput),
    ("mkdir", Applies::FileOutput),
];

fn value_name(value: impl ValueEnum) -> String {
    value
        .to_possible_value()
        .map(|value| value.get_name().to_owned())
        .unwrap_or_default()
}

fn long_name(id: &str) -> String {
    Cli::command()
        .get_arguments()
        .find(|arg| arg.get_id() == id)
        .and_then(|arg| arg.get_long())
        .map_or_else(|| id.to_owned(), |long| format!("--{long}"))
}

fn ignored_reason(cli: &Cli, option: &str, applies: &Applies) -> Option<String> {
    let gradient = !cli.background_gradient.is_empty();
    match applies {
        Applies::Layouts(layouts) if !layouts.contains(&cli.layout) => Some(format!(
            "{option} is not used by the {} layout",
            value_name(cli.layout)
        )),
        Applies::Labels if cli.label.is_empty() => {
            Some(format!("{option} is only used with --label"))
        }
        Applies::Gradient | Applies::LinearGradient if !gradient => {
            Some(format!("{option} is only used with --background-gradient"))
        }
        Applies::LinearGradient if cli.gradient_kind == CliGradientKind::Radial => {
            Some(format!("{option} is not used by radial gradients"))
        }
        Applies::JpegOutput | Applies::Render | Applies::FileOutput if cli.print_spec => {
            Some(format!("{option} is not used with --print-spec"))
        }
        Applies::JpegOutput => match io::output_format(&cli.output, cli.format) {
            Ok(ExportFormat::Png) => Some(format!("{option} is not used for PNG output")),
            _ => None,
        },
        Applies::FileOutput if io::is_stdio(&cli.output) => {
            Some(format!("{option} is not used with -o -"))
        }
        _ => None,
    }
}

/// Explicitly passed options that have no effect on this invocation.
pub(crate) fn option_warnings(cli: &Cli) -> Vec<Warning> {
    RULES
        .iter()
        .filter(|(id, _)| cli.explicit.iter().any(|explicit| explicit == id))
        .filter_map(|(id, applies)| {
            let option = long_name(id);
            ignored_reason(cli, &option, applies).map(|reason| Warning {
                code: "option_ignored",
                reason,
                context: json!({ "option": option }),
            })
        })
        .collect()
}

fn label_warning(warning: &LabelWarning, labels: &[String]) -> Warning {
    match warning {
        LabelWarning::Overflow {
            index,
            tab_width,
            available_width,
        } => {
            let label = labels.get(*index).map_or("", String::as_str);
            Warning {
                code: "label_overflow",
                reason: format!(
                    "label {label:?} (index {index}) is {tab_width} px wide but only {available_width} px are available; it extends past its image"
                ),
                context: json!({
                    "index": index,
                    "label": label,
                    "tab_width": tab_width,
                    "available_width": available_width
                }),
            }
        }
        LabelWarning::MissingGlyphs { index, characters } => {
            let label = labels.get(*index).map_or("", String::as_str);
            let characters = characters.iter().collect::<String>();
            Warning {
                code: "missing_glyphs",
                reason: format!(
                    "label {label:?} (index {index}) contains characters the font cannot draw: {characters}"
                ),
                context: json!({
                    "index": index,
                    "label": label,
                    "characters": characters
                }),
            }
        }
    }
}

pub(crate) fn label_warnings(warnings: &[LabelWarning], labels: &[String]) -> Vec<Warning> {
    warnings
        .iter()
        .map(|warning| label_warning(warning, labels))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Cli {
        crate::parse_cli(
            std::iter::once("stitchr")
                .chain(args.iter().copied())
                .map(std::ffi::OsString::from)
                .collect(),
        )
        .unwrap()
    }

    fn ignored(args: &[&str]) -> Vec<(String, String)> {
        option_warnings(&cli(args))
            .into_iter()
            .map(|warning| {
                assert_eq!(warning.code, "option_ignored");
                (
                    warning.context["option"].as_str().unwrap().to_owned(),
                    warning.reason,
                )
            })
            .collect()
    }

    fn options(args: &[&str]) -> Vec<String> {
        ignored(args)
            .into_iter()
            .map(|(option, _)| option)
            .collect()
    }

    #[test]
    fn defaults_never_warn() {
        assert_eq!(options(&["a.png"]), Vec::<String>::new());
        assert_eq!(
            options(&["a.png", "--layout", "split"]),
            Vec::<String>::new()
        );
        assert_eq!(options(&["a.png", "--print-spec"]), Vec::<String>::new());
    }

    #[test]
    fn options_that_apply_do_not_warn() {
        assert_eq!(
            options(&[
                "a.png",
                "--layout",
                "grid",
                "--columns",
                "3",
                "--spacing",
                "4",
                "--alignment",
                "center",
                "--label",
                "A",
                "--border-width",
                "3",
                "--border-color",
                "red",
                "--label-size",
                "12",
                "--background-gradient",
                "red",
                "blue",
                "--gradient-kind",
                "linear",
                "--gradient-angle",
                "90",
                "-o",
                "out.jpg",
                "--jpeg-quality",
                "90",
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn split_reports_grid_and_stack_options_with_a_reason() {
        assert_eq!(
            ignored(&[
                "a.png",
                "--layout",
                "split",
                "--spacing",
                "40",
                "--columns",
                "3"
            ]),
            [
                (
                    "--columns".to_owned(),
                    "--columns is not used by the split layout".to_owned()
                ),
                (
                    "--spacing".to_owned(),
                    "--spacing is not used by the split layout".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn columns_only_apply_to_grid() {
        for layout in ["horizontal", "vertical", "freeform", "split"] {
            assert_eq!(
                options(&["a.png", "--layout", layout, "--columns", "2"]),
                ["--columns"],
                "{layout}"
            );
        }
    }

    #[test]
    fn spacing_and_alignment_are_ignored_by_freeform() {
        assert_eq!(
            options(&[
                "a.png",
                "--layout",
                "freeform",
                "--position",
                "0",
                "0",
                "--spacing",
                "1",
                "--alignment",
                "end",
            ]),
            ["--spacing", "--alignment"]
        );
    }

    #[test]
    fn explicit_value_equal_to_the_default_still_warns() {
        assert_eq!(
            options(&["a.png", "--layout", "split", "--spacing", "0"]),
            ["--spacing"]
        );
    }

    #[test]
    fn position_only_applies_to_freeform() {
        assert_eq!(options(&["a.png", "--position", "1", "2"]), ["--position"]);
    }

    #[test]
    fn divider_only_applies_to_split() {
        assert_eq!(
            ignored(&["a.png", "--divider", "4"]),
            [(
                "--divider".to_owned(),
                "--divider is not used by the grid layout".to_owned()
            )]
        );
    }

    #[test]
    fn label_styling_without_labels_is_reported() {
        assert_eq!(
            ignored(&[
                "a.png",
                "--border-width",
                "4",
                "--border-color",
                "red",
                "--label-size",
                "9"
            ])
            .into_iter()
            .map(|(_, reason)| reason)
            .collect::<Vec<_>>(),
            [
                "--border-width is only used with --label",
                "--border-color is only used with --label",
                "--label-size is only used with --label",
            ]
        );
    }

    #[test]
    fn gradient_options_without_a_gradient_are_reported() {
        assert_eq!(
            ignored(&[
                "a.png",
                "--gradient-kind",
                "radial",
                "--gradient-angle",
                "90"
            ])
            .into_iter()
            .map(|(_, reason)| reason)
            .collect::<Vec<_>>(),
            [
                "--gradient-kind is only used with --background-gradient",
                "--gradient-angle is only used with --background-gradient",
            ]
        );
    }

    #[test]
    fn gradient_angle_is_ignored_by_radial_gradients() {
        assert_eq!(
            ignored(&[
                "a.png",
                "--background-gradient",
                "red",
                "blue",
                "--gradient-kind",
                "radial",
                "--gradient-angle",
                "90",
            ]),
            [(
                "--gradient-angle".to_owned(),
                "--gradient-angle is not used by radial gradients".to_owned()
            )]
        );
    }

    #[test]
    fn jpeg_quality_is_ignored_for_png_output() {
        assert_eq!(
            ignored(&["a.png", "--jpeg-quality", "90"]),
            [(
                "--jpeg-quality".to_owned(),
                "--jpeg-quality is not used for PNG output".to_owned()
            )]
        );
        assert_eq!(
            options(&[
                "a.png",
                "--jpeg-quality",
                "90",
                "--format",
                "png",
                "-o",
                "x.jpg"
            ]),
            ["--jpeg-quality"]
        );
    }

    #[test]
    fn jpeg_quality_applies_to_jpeg_extensions_and_format_overrides() {
        assert_eq!(
            options(&["a.png", "--jpeg-quality", "90", "-o", "out.JPEG"]),
            Vec::<String>::new()
        );
        assert_eq!(
            options(&[
                "a.png",
                "--jpeg-quality",
                "90",
                "--format",
                "jpeg",
                "-o",
                "x.png"
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn jpeg_quality_with_an_unknown_output_extension_is_left_to_the_format_error() {
        assert_eq!(
            options(&["a.png", "--jpeg-quality", "90", "-o", "out.webp"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn output_options_are_ignored_when_only_printing_the_spec() {
        assert_eq!(
            ignored(&[
                "a.png",
                "--print-spec",
                "-o",
                "out.jpg",
                "--format",
                "jpeg",
                "--jpeg-quality",
                "50",
            ])
            .into_iter()
            .map(|(_, reason)| reason)
            .collect::<Vec<_>>(),
            [
                "--jpeg-quality is not used with --print-spec",
                "--output is not used with --print-spec",
                "--format is not used with --print-spec",
            ]
        );
    }

    #[test]
    fn file_output_flags_apply_to_file_renders() {
        assert_eq!(
            options(&[
                "a.png",
                "--force",
                "--no-clobber",
                "--mkdir",
                "-o",
                "dir/out.png"
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn file_output_flags_are_ignored_for_stdout() {
        assert_eq!(
            ignored(&[
                "a.png",
                "-o",
                "-",
                "--format",
                "png",
                "--force",
                "--no-clobber",
                "--mkdir"
            ])
            .into_iter()
            .map(|(_, reason)| reason)
            .collect::<Vec<_>>(),
            [
                "--force is not used with -o -",
                "--no-clobber is not used with -o -",
                "--mkdir is not used with -o -",
            ]
        );
    }

    #[test]
    fn file_output_flags_are_ignored_when_printing_the_spec() {
        assert_eq!(options(&["a.png", "--print-spec", "--mkdir"]), ["--mkdir"]);
    }

    #[test]
    fn option_warning_json_has_code_option_and_reason() {
        let warning =
            option_warnings(&cli(&["a.png", "--layout", "split", "--spacing", "4"])).remove(0);

        assert_eq!(
            warning.json(),
            json!({
                "code": "option_ignored",
                "option": "--spacing",
                "reason": "--spacing is not used by the split layout"
            })
        );
    }

    #[test]
    fn overflow_warning_names_the_label_and_widths() {
        let warnings = label_warnings(
            &[LabelWarning::Overflow {
                index: 1,
                tab_width: 90,
                available_width: 24,
            }],
            &["A".into(), "Long".into()],
        );

        assert_eq!(
            warnings[0].json(),
            json!({
                "code": "label_overflow",
                "index": 1,
                "label": "Long",
                "tab_width": 90,
                "available_width": 24,
                "reason": "label \"Long\" (index 1) is 90 px wide but only 24 px are available; it extends past its image"
            })
        );
    }

    #[test]
    fn missing_glyph_warning_lists_the_characters() {
        let warnings = label_warnings(
            &[LabelWarning::MissingGlyphs {
                index: 0,
                characters: vec!['日', '本'],
            }],
            &["日本".into()],
        );

        assert_eq!(
            warnings[0].json(),
            json!({
                "code": "missing_glyphs",
                "index": 0,
                "label": "日本",
                "characters": "日本",
                "reason": "label \"日本\" (index 0) contains characters the font cannot draw: 日本"
            })
        );
    }
}
