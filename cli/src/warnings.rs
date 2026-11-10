use crate::{
    io,
    options::{value_name, Applies, RULES},
    Cli, CliGradientKind, CliLayout,
};
use clap::CommandFactory;
use serde_json::{json, Value};
use stitchr_core::{CompositionSpec, ExportFormat, FitTarget, ImageMetadata, LabelWarning};

pub(crate) const WARNING_CODES: &[(&str, &str)] = &[
    (
        "option_ignored",
        "An explicitly passed option has no effect on this invocation.",
    ),
    (
        "label_overflow",
        "A label tab is wider than its image or split band.",
    ),
    (
        "missing_glyphs",
        "The label font cannot draw some label characters.",
    ),
    (
        "format_mismatch",
        "--format writes a different format than the output file extension suggests.",
    ),
    (
        "tiny_images",
        "--max-width/--max-height shrinks an image below 16 px on a side, so it will be hard to see.",
    ),
    (
        "fit_not_applied",
        "An image is already smaller than --fit-height/--fit-width and is left unchanged, because images are never enlarged.",
    ),
    (
        "annotation_clipped",
        "A box or arrow reaches past the canvas edge, so part or all of it is not drawn.",
    ),
];

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
        Applies::Annotations if cli.boxes.is_empty() && cli.arrows.is_empty() => {
            Some(format!("{option} is only used with --box or --arrow"))
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
            Ok(format) if format != ExportFormat::Jpeg => Some(format!(
                "{option} is not used for {} output",
                display_name(format)
            )),
            _ => None,
        },
        Applies::FileOutput if io::is_stdio(&cli.output) => {
            Some(format!("{option} is not used with -o -"))
        }
        _ => None,
    }
}

fn display_name(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Png => "PNG",
        ExportFormat::Jpeg => "JPEG",
        ExportFormat::Webp => "WebP",
    }
}

/// Options that only shape a composition built from arguments, so a `--spec` file overrides them.
const COMPOSITION_OPTIONS: &[&str] = &[
    "layout",
    "columns",
    "spacing",
    "alignment",
    "background_color",
    "background_gradient",
    "gradient_kind",
    "gradient_angle",
    "position",
    "divider",
    "label",
    "border_width",
    "border_color",
    "label_size",
    "label_position",
    "label_color",
    "font",
    "fit_height",
    "fit_width",
    "fit",
    "max_width",
    "max_height",
    "box",
    "arrow",
    "annotation_color",
    "annotation_width",
];

fn explicit(cli: &Cli, id: &str) -> bool {
    cli.explicit.iter().any(|explicit| explicit == id)
}

fn spec_warnings(cli: &Cli) -> Vec<Warning> {
    COMPOSITION_OPTIONS
        .iter()
        .filter(|id| cli.spec.is_some() && explicit(cli, id))
        .map(|id| {
            let option = long_name(id);
            Warning {
                code: "option_ignored",
                reason: format!("{option} is not used with --spec"),
                context: json!({ "option": option }),
            }
        })
        .collect()
}

/// Explicitly passed options that have no effect on this invocation.
pub(crate) fn option_warnings(cli: &Cli) -> Vec<Warning> {
    let overridden = |id: &str| cli.spec.is_some() && COMPOSITION_OPTIONS.contains(&id);
    let mut warnings = spec_warnings(cli);
    warnings.extend(
        RULES
            .iter()
            .filter(|rule| explicit(cli, rule.id) && !overridden(rule.id))
            .filter_map(|rule| {
                let option = long_name(rule.id);
                ignored_reason(cli, &option, &rule.applies).map(|reason| Warning {
                    code: "option_ignored",
                    reason,
                    context: json!({ "option": option }),
                })
            }),
    );
    warnings
}

/// `--format` that contradicts a known output extension, e.g. JPEG data in `out.png`.
pub(crate) fn format_warnings(cli: &Cli) -> Vec<Warning> {
    if cli.print_spec || io::is_stdio(&cli.output) {
        return Vec::new();
    }
    let (Some(_), Ok(chosen), Ok(implied)) = (
        cli.format,
        io::output_format(&cli.output, cli.format),
        io::output_format(&cli.output, None),
    ) else {
        return Vec::new();
    };
    if chosen == implied {
        return Vec::new();
    }
    let path = cli.output.to_string_lossy();
    vec![Warning {
        code: "format_mismatch",
        reason: format!(
            "--format {} writes {} data to {path}, whose extension suggests {}",
            io::format_name(chosen),
            display_name(chosen),
            display_name(implied)
        ),
        context: json!({
            "option": "--format",
            "format": io::format_name(chosen),
            "path": path,
            "extension_format": io::format_name(implied)
        }),
    }]
}

/// Images under this many pixels on a side are too small to show their content.
const TINY_SIDE: u32 = 16;

/// Images that `--max-width`/`--max-height` shrink to a barely visible size.
pub(crate) fn scale_warnings(
    spec: &CompositionSpec,
    metadata: &[ImageMetadata],
    scale: f64,
) -> Vec<Warning> {
    if scale >= 1.0 {
        return Vec::new();
    }
    spec.images
        .iter()
        .zip(metadata)
        .enumerate()
        .filter(|(_, (placement, _))| placement.width.min(placement.height) < TINY_SIDE)
        .map(|(index, (placement, source))| Warning {
            code: "tiny_images",
            reason: format!(
                "--max-width/--max-height shrinks image {index} ({}) from {}x{} to {}x{} px (scale {scale:.3}); it will be hard to see",
                source.name, source.width, source.height, placement.width, placement.height
            ),
            context: json!({
                "index": index,
                "width": placement.width,
                "height": placement.height,
                "source_width": source.width,
                "source_height": source.height,
                "scale": scale,
                "min_side": TINY_SIDE
            }),
        })
        .collect()
}

fn fit_warning(
    option: &str,
    target: Option<FitTarget>,
    dimension: &str,
    length: impl Fn(&ImageMetadata) -> u32,
    metadata: &[ImageMetadata],
) -> Vec<Warning> {
    let Some(FitTarget::Pixels(target)) = target else {
        return Vec::new();
    };
    metadata
        .iter()
        .enumerate()
        .filter(|(_, image)| length(image) < target)
        .map(|(index, image)| Warning {
            code: "fit_not_applied",
            reason: format!(
                "{option} {target} does not apply to image {index} ({} is {} px {dimension}); images are never enlarged",
                image.name,
                length(image)
            ),
            context: json!({
                "option": option,
                "index": index,
                "size": length(image),
                "target": target
            }),
        })
        .collect()
}

/// Images that `--fit-height`/`--fit-width` leave unchanged because they are already smaller.
pub(crate) fn fit_warnings(cli: &Cli, metadata: &[ImageMetadata]) -> Vec<Warning> {
    if cli.spec.is_some() || cli.layout == CliLayout::Split {
        return Vec::new();
    }
    let mut warnings = fit_warning(
        "--fit-height",
        cli.fit_height,
        "tall",
        |image| image.height,
        metadata,
    );
    warnings.extend(fit_warning(
        "--fit-width",
        cli.fit_width,
        "wide",
        |image| image.width,
        metadata,
    ));
    warnings
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

/// Boxes and arrows that reach past the canvas, so part or all of them is cut off.
pub(crate) fn annotation_warnings(spec: &CompositionSpec) -> Vec<Warning> {
    let (width, height) = (spec.width, spec.height);
    stitchr_core::annotation_warnings(&spec.annotations, (width, height))
        .into_iter()
        .map(|clipped| {
            let (kind, index) = (clipped.kind, clipped.index);
            let reason = if clipped.visible {
                format!(
                    "{kind} {index} reaches past the {width}x{height} canvas and is partly cut off"
                )
            } else {
                format!("{kind} {index} lies outside the {width}x{height} canvas and is not drawn")
            };
            Warning {
                code: "annotation_clipped",
                reason,
                context: json!({
                    "annotation_index": clipped.index,
                    "shape": clipped.kind,
                    "canvas_width": width,
                    "canvas_height": height,
                    "visible": clipped.visible
                }),
            }
        })
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
    fn every_emitted_warning_code_is_listed() {
        let source = include_str!("warnings.rs");
        let emitted = source
            .split("code: \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .filter(|code| !code.contains(' '))
            .collect::<Vec<_>>();

        assert!(emitted.len() >= 5);
        for code in emitted {
            assert!(
                WARNING_CODES.iter().any(|(listed, _)| *listed == code),
                "{code}"
            );
        }
    }

    #[test]
    fn every_warning_code_is_documented_for_agents() {
        let guide = include_str!("../../docs/agents.md");

        for (code, _) in WARNING_CODES {
            assert!(guide.contains(&format!("| `{code}`")), "{code}");
        }
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
            options(&["a.png", "--jpeg-quality", "90", "-o", "out.bmp"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn jpeg_quality_is_ignored_for_webp_output() {
        assert_eq!(
            ignored(&["a.png", "--jpeg-quality", "90", "-o", "out.webp"]),
            [(
                "--jpeg-quality".to_owned(),
                "--jpeg-quality is not used for WebP output".to_owned()
            )]
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
    fn composition_options_are_ignored_with_a_spec_file() {
        assert_eq!(
            ignored(&[
                "--spec",
                "s.json",
                "--layout",
                "split",
                "--background",
                "red",
                "--label",
                "A"
            ]),
            [
                (
                    "--layout".to_owned(),
                    "--layout is not used with --spec".to_owned()
                ),
                (
                    "--background".to_owned(),
                    "--background is not used with --spec".to_owned()
                ),
                (
                    "--label".to_owned(),
                    "--label is not used with --spec".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn spec_file_overrides_layout_rules_without_duplicate_warnings() {
        assert_eq!(
            options(&["--spec", "s.json", "--layout", "split", "--spacing", "4"]),
            ["--layout", "--spacing"]
        );
    }

    #[test]
    fn output_rules_still_apply_with_a_spec_file() {
        assert_eq!(
            ignored(&["--spec", "s.json", "--jpeg-quality", "80"]),
            [(
                "--jpeg-quality".to_owned(),
                "--jpeg-quality is not used for PNG output".to_owned()
            )]
        );
        assert_eq!(
            options(&["--spec", "s.json", "-o", "out.jpg", "--mkdir"]),
            Vec::<String>::new()
        );
    }

    fn image(name: &str, width: u32, height: u32) -> ImageMetadata {
        ImageMetadata {
            name: name.into(),
            width,
            height,
        }
    }

    #[test]
    fn fit_options_apply_outside_split() {
        assert_eq!(
            options(&[
                "a.png",
                "--layout",
                "horizontal",
                "--fit-height",
                "min",
                "--fit-width",
                "9"
            ]),
            Vec::<String>::new()
        );
        assert_eq!(
            options(&["a.png", "--layout", "split", "--fit-height", "min"]),
            ["--fit-height"]
        );
    }

    #[test]
    fn split_fit_mode_only_applies_to_split() {
        assert_eq!(
            ignored(&["a.png", "--fit", "cover"]),
            [(
                "--fit".to_owned(),
                "--fit is not used by the grid layout".to_owned()
            )]
        );
        assert_eq!(
            options(&["a.png", "--layout", "split", "--fit", "contain"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn max_size_options_always_apply() {
        assert_eq!(
            options(&[
                "a.png",
                "--layout",
                "split",
                "--max-width",
                "10",
                "--max-height",
                "10"
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn fit_options_are_ignored_with_a_spec_file() {
        assert_eq!(
            options(&["--spec", "s.json", "--fit-height", "10", "--max-width", "5"]),
            ["--fit-height", "--max-width"]
        );
    }

    #[test]
    fn fit_height_reports_images_already_shorter_than_the_target() {
        let cli = cli(&[
            "a.png",
            "b.png",
            "--layout",
            "horizontal",
            "--fit-height",
            "400",
        ]);
        let warnings = fit_warnings(&cli, &[image("a.png", 800, 600), image("b.png", 500, 300)]);

        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0].json(),
            json!({
                "code": "fit_not_applied",
                "option": "--fit-height",
                "index": 1,
                "size": 300,
                "target": 400,
                "reason": "--fit-height 400 does not apply to image 1 (b.png is 300 px tall); images are never enlarged"
            })
        );
    }

    #[test]
    fn fit_width_reports_narrower_images() {
        let cli = cli(&["a.png", "--layout", "vertical", "--fit-width", "100"]);
        let warnings = fit_warnings(&cli, &[image("a.png", 99, 600)]);

        assert_eq!(warnings[0].context["option"], "--fit-width");
        assert_eq!(warnings[0].context["size"], 99);
    }

    #[test]
    fn fit_min_and_exact_matches_never_warn() {
        let images = [image("a.png", 100, 100), image("b.png", 50, 50)];

        assert!(fit_warnings(&cli(&["a.png", "b.png", "--fit-height", "min"]), &images).is_empty());
        assert!(fit_warnings(&cli(&["a.png", "b.png", "--fit-height", "50"]), &images).is_empty());
    }

    #[test]
    fn fit_warnings_are_skipped_where_fit_options_are_ignored() {
        let images = [image("a.png", 10, 10)];

        assert!(fit_warnings(
            &cli(&["a.png", "--layout", "split", "--fit-height", "50"]),
            &images
        )
        .is_empty());
        assert!(
            fit_warnings(&cli(&["--spec", "s.json", "--fit-height", "50"]), &images).is_empty()
        );
    }

    #[test]
    fn format_contradicting_the_extension_is_reported() {
        let warnings = format_warnings(&cli(&["a.png", "--format", "jpeg", "-o", "out.png"]));

        assert_eq!(
            warnings.iter().map(Warning::json).collect::<Vec<_>>(),
            [json!({
                "code": "format_mismatch",
                "option": "--format",
                "format": "jpeg",
                "path": "out.png",
                "extension_format": "png",
                "reason": "--format jpeg writes JPEG data to out.png, whose extension suggests PNG"
            })]
        );
    }

    #[test]
    fn png_format_into_a_jpeg_extension_is_reported() {
        let warnings = format_warnings(&cli(&["a.png", "--format", "png", "-o", "photo.JPG"]));

        assert_eq!(warnings[0].context["extension_format"], "jpeg");
    }

    #[test]
    fn format_matching_the_extension_is_not_reported() {
        for args in [
            ["a.png", "--format", "jpeg", "-o", "out.jpeg"],
            ["a.png", "--format", "jpg", "-o", "out.jpg"],
            ["a.png", "--format", "png", "-o", "out.PNG"],
        ] {
            assert!(format_warnings(&cli(&args)).is_empty(), "{args:?}");
        }
    }

    #[test]
    fn format_for_an_unknown_or_missing_extension_is_not_reported() {
        assert!(format_warnings(&cli(&["a.png", "--format", "jpeg", "-o", "out.img"])).is_empty());
        assert!(format_warnings(&cli(&["a.png", "--format", "png", "-o", "out"])).is_empty());
    }

    #[test]
    fn format_mismatch_is_not_reported_without_a_file_output() {
        assert!(format_warnings(&cli(&["a.png", "--format", "jpeg", "-o", "-"])).is_empty());
        assert!(format_warnings(&cli(&[
            "a.png",
            "--format",
            "jpeg",
            "-o",
            "out.png",
            "--print-spec"
        ]))
        .is_empty());
    }

    #[test]
    fn extension_alone_never_reports_a_mismatch() {
        assert!(format_warnings(&cli(&["a.png", "-o", "out.jpg"])).is_empty());
    }

    fn scaled_spec(sizes: &[(u32, u32)]) -> CompositionSpec {
        stitchr_core::compose_spec(
            &sizes
                .iter()
                .map(|&(width, height)| image("a.png", width, height))
                .collect::<Vec<_>>(),
            &stitchr_core::CompositionParams {
                layout: stitchr_core::SpecLayout::Grid {
                    columns: 2,
                    spacing: 0,
                    alignment: stitchr_core::Alignment::Start,
                },
                background_color: stitchr_core::RgbColor::default(),
                background_gradient: None,
                frame: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn annotation_styling_without_shapes_is_reported() {
        assert_eq!(
            ignored(&["a.png", "--annotation-color", "blue"]),
            [(
                "--annotation-color".to_owned(),
                "--annotation-color is only used with --box or --arrow".to_owned()
            )]
        );
        assert_eq!(
            options(&[
                "a.png",
                "--annotation-width",
                "2",
                "--arrow",
                "0",
                "0",
                "5",
                "5"
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn annotations_are_ignored_with_a_spec_file() {
        assert_eq!(
            options(&["--spec", "s.json", "--box", "0", "0", "2", "2"]),
            ["--box"]
        );
    }

    #[test]
    fn annotations_past_the_canvas_are_reported() {
        let mut spec = scaled_spec(&[(10, 10)]);
        spec.annotations = cli(&[
            "a.png", "--box", "5", "5", "10", "2", "--arrow", "20", "20", "30", "30",
        ])
        .annotations();

        assert_eq!(
            annotation_warnings(&spec)
                .iter()
                .map(Warning::json)
                .collect::<Vec<_>>(),
            [
                json!({
                    "code": "annotation_clipped",
                    "annotation_index": 0,
                    "shape": "box",
                    "canvas_width": 10,
                    "canvas_height": 10,
                    "visible": true,
                    "reason": "box 0 reaches past the 10x10 canvas and is partly cut off"
                }),
                json!({
                    "code": "annotation_clipped",
                    "annotation_index": 1,
                    "shape": "arrow",
                    "canvas_width": 10,
                    "canvas_height": 10,
                    "visible": false,
                    "reason": "arrow 1 lies outside the 10x10 canvas and is not drawn"
                }),
            ]
        );
    }

    #[test]
    fn heavy_scaling_reports_images_below_sixteen_pixels() {
        let spec = scaled_spec(&[(10, 3), (40, 40)]);
        let warnings = scale_warnings(
            &spec,
            &[image("a.png", 640, 200), image("b.png", 2560, 2560)],
            0.015625,
        );

        assert_eq!(
            warnings.iter().map(Warning::json).collect::<Vec<_>>(),
            [json!({
                "code": "tiny_images",
                "index": 0,
                "width": 10,
                "height": 3,
                "source_width": 640,
                "source_height": 200,
                "scale": 0.015625,
                "min_side": 16,
                "reason": "--max-width/--max-height shrinks image 0 (a.png) from 640x200 to 10x3 px (scale 0.016); it will be hard to see"
            })]
        );
    }

    #[test]
    fn images_at_the_threshold_do_not_warn() {
        let spec = scaled_spec(&[(16, 16)]);

        assert!(scale_warnings(&spec, &[image("a.png", 160, 160)], 0.1).is_empty());
    }

    #[test]
    fn small_inputs_that_were_not_scaled_do_not_warn() {
        let spec = scaled_spec(&[(4, 4)]);

        assert!(scale_warnings(&spec, &[image("a.png", 4, 4)], 1.0).is_empty());
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
