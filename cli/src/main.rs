mod composer;
mod error;
mod io;
mod schema;

use clap::{ArgAction, Parser, ValueEnum};
use composer::{CliComposeParams, CliComposer};
use error::CliError;
use io::is_stdio;
use serde_json::{json, Value};
use std::{path::PathBuf, process::ExitCode};
use stitchr_core::{render_spec, CompositionSpec, LoadedImage, RgbColor, DEFAULT_JPEG_QUALITY};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliFormat {
    Png,
    #[value(alias = "jpg")]
    Jpeg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliLayout {
    Grid,
    Horizontal,
    Vertical,
    Freeform,
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliAlignment {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliGradientKind {
    Linear,
    Radial,
}

#[derive(Debug, Parser)]
#[command(
    name = "stitchr",
    version,
    about = "Combine PNG and JPEG images into one image",
    long_about = "Arrange PNG/JPEG images in a grid, horizontal or vertical stack, at explicit freeform positions, or as diagonal split slices of same-size images. Output format is selected by the output file extension.",
    after_help = "Examples:\n  stitchr img1.png img2.png --layout grid --spacing 10\n  stitchr img1.png img2.jpg --layout horizontal --alignment center -o combined.jpg\n  stitchr img1.png img2.png --layout freeform --position 0 0 --position 300 80 -o combined.png\n  stitchr light.png dark.png --layout split --divider 4 --background white -o split.png\n  stitchr before.png after.png --layout horizontal --spacing 16 --background white --label Before --label After -o labeled.png\n  stitchr img1.png img2.png --spacing 40 --background-gradient '#ff7e5f' '#feb47b' --gradient-angle 135 -o gradient.png\n  stitchr img1.png img2.png --background rebeccapurple --print-spec"
)]
struct Cli {
    #[arg(required_unless_present = "help_json", num_args = 1.., value_name = "IMAGE", help = "Input PNG/JPEG paths, or - to read one image from stdin (one or more; required unless --help-json/--schema)")]
    images: Vec<PathBuf>,

    #[arg(
        long,
        value_enum,
        default_value = "grid",
        help = "Layout: grid, horizontal stack, vertical stack, freeform, or diagonal split of same-size images"
    )]
    layout: CliLayout,

    #[arg(long, default_value_t = 2, allow_negative_numbers = true, value_parser = clap::value_parser!(u32).range(1..), value_name = "COUNT", help = "Grid columns (positive integer; only for grid)")]
    columns: u32,

    #[arg(
        long,
        default_value_t = 0,
        value_name = "PIXELS",
        allow_negative_numbers = true,
        value_parser = non_negative,
        help = "Space between images and around the canvas (nonnegative integer; grid/stack)"
    )]
    spacing: u32,

    #[arg(
        long,
        value_enum,
        default_value = "start",
        help = "Align images within cells or across a stack: start, center, end"
    )]
    alignment: CliAlignment,

    #[arg(long = "background", default_value_t = RgbColor::default(), value_name = "COLOR", help = "Opaque background: RGB #RRGGBB, rgb(r,g,b), or common color name")]
    background_color: RgbColor,

    #[arg(long, action = ArgAction::Set, num_args = 2, value_names = ["FROM", "TO"], conflicts_with = "background_color", help = "Gradient background from one color to another, instead of --background; split dividers use FROM")]
    background_gradient: Vec<RgbColor>,

    #[arg(
        long,
        value_enum,
        default_value = "linear",
        help = "Gradient shape: linear, or radial from the canvas center (only with --background-gradient)"
    )]
    gradient_kind: CliGradientKind,

    #[arg(long, default_value_t = 180, allow_negative_numbers = true, value_parser = clap::value_parser!(u32).range(0..360), value_name = "DEGREES", help = "Linear gradient direction like CSS: 0 up, 90 right, 180 down (integer 0-359; only for linear gradients)")]
    gradient_angle: u32,

    #[arg(long, action = ArgAction::Append, num_args = 2, value_names = ["X", "Y"], allow_negative_numbers = true, value_parser = non_negative, help = "Freeform top-left coordinates (nonnegative integers; repeat once per image)")]
    position: Vec<u32>,

    #[arg(
        long,
        default_value_t = 0,
        value_name = "PIXELS",
        allow_negative_numbers = true,
        value_parser = non_negative,
        help = "Width of the background-colored line between split images (nonnegative integer; only for split)"
    )]
    divider: u32,

    #[arg(long, action = ArgAction::Append, value_name = "TEXT", help = "Header label for each image, in input order (repeat once per image); adds a border around each image. Use \"\" for a border without a label")]
    label: Vec<String>,

    #[arg(
        long,
        default_value_t = 2,
        value_name = "PIXELS",
        allow_negative_numbers = true,
        value_parser = non_negative,
        help = "Border width around labeled images (nonnegative integer; only with --label)"
    )]
    border_width: u32,

    #[arg(long, default_value_t = RgbColor { red: 128, green: 128, blue: 128 }, value_name = "COLOR", help = "Border and label tab color: RGB #RRGGBB, rgb(r,g,b), or common color name (only with --label)")]
    border_color: RgbColor,

    #[arg(long, default_value_t = 20, allow_negative_numbers = true, value_parser = clap::value_parser!(u32).range(1..), value_name = "PIXELS", help = "Label font size (positive integer; only with --label)")]
    label_size: u32,
    #[arg(
        long,
        short,
        default_value = "stitched.png",
        value_name = "FILE",
        help = "Output file (.png, .jpg, or .jpeg; default: stitched.png), or - for stdout (requires --format; not with --json)"
    )]
    output: PathBuf,

    #[arg(
        long,
        value_enum,
        value_name = "FORMAT",
        help = "Output format: png or jpeg; overrides the output extension (required with -o -)"
    )]
    format: Option<CliFormat>,

    #[arg(long, default_value_t = DEFAULT_JPEG_QUALITY, allow_negative_numbers = true, value_parser = clap::value_parser!(u8).range(1..=100), value_name = "QUALITY", help = "JPEG encoding quality (integer 1-100; only for .jpg/.jpeg output)")]
    jpeg_quality: u8,

    #[arg(long = "help-json", visible_alias = "schema", action = ArgAction::SetTrue, help = "Print the full CLI specification as JSON (boolean; default: false)")]
    help_json: bool,

    #[arg(long = "print-spec", action = ArgAction::SetTrue, help = "Print the composition spec as JSON after loading inputs (boolean; default: false)")]
    print_spec: bool,

    #[arg(long, action = ArgAction::SetTrue, help = "Print versioned JSON results and errors to stdout")]
    json: bool,
}

fn non_negative(value: &str) -> Result<u32, String> {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    value
        .parse::<u32>()
        .map_err(|_| match value.strip_prefix('-') {
            Some(magnitude) if digits(magnitude) => "must be ≥ 0".to_owned(),
            _ if digits(value.strip_prefix('+').unwrap_or(value)) => {
                format!("must be at most {}", u32::MAX)
            }
            _ => "must be an integer ≥ 0".to_owned(),
        })
}

impl Cli {
    fn compose_params(&self) -> CliComposeParams<'_> {
        CliComposeParams {
            layout: self.layout,
            columns: self.columns,
            spacing: self.spacing,
            alignment: self.alignment,
            background_color: self.background_color,
            positions: &self.position,
            divider: self.divider,
            labels: &self.label,
            border_width: self.border_width,
            border_color: self.border_color,
            label_size: self.label_size,
            background_gradient: &self.background_gradient,
            gradient_kind: self.gradient_kind,
            gradient_angle: self.gradient_angle,
        }
    }
}

#[derive(Debug)]
enum Success {
    Help(String),
    Version,
    Schema(Value),
    Spec(Value),
    Rendered(io::ExportResult),
}

impl Success {
    fn json(&self) -> Value {
        let mut response = match self {
            Self::Help(help) => json!({ "help": help }),
            Self::Version => json!({ "version": env!("CARGO_PKG_VERSION") }),
            Self::Schema(schema) => json!({ "schema": schema }),
            Self::Spec(spec) => json!({ "spec": spec }),
            Self::Rendered(result) => json!({
                "output": result.output.to_string_lossy(),
                "width": result.width,
                "height": result.height,
                "format": result.format
            }),
        };
        response["schema_version"] = json!(SCHEMA_VERSION);
        response["ok"] = json!(true);
        response
    }

    fn human(&self) -> Option<String> {
        match self {
            Self::Help(help) => Some(help.clone()),
            Self::Version => Some(format!("stitchr {}", env!("CARGO_PKG_VERSION"))),
            Self::Schema(schema) => Some(schema.to_string()),
            Self::Spec(spec) => Some(format!("{spec:#}")),
            Self::Rendered(result) if is_stdio(&result.output) => None,
            Self::Rendered(result) => Some(result.output.display().to_string()),
        }
    }
}

fn compose_loaded_images(
    loaded: &[LoadedImage],
    params: CliComposeParams<'_>,
) -> Result<CompositionSpec, CliError> {
    let metadata = loaded
        .iter()
        .map(|image| image.metadata.clone())
        .collect::<Vec<_>>();
    CliComposer::compose(&metadata, params).map_err(CliError::from)
}

fn spec_json(spec: &CompositionSpec) -> Result<Value, CliError> {
    let mut value = serde_json::to_value(spec).map_err(|error| {
        CliError::new("serialization_error", format!("cannot print spec: {error}"))
    })?;
    value["schema_version"] = json!(SCHEMA_VERSION);
    Ok(value)
}

fn execute(cli: Cli) -> Result<Success, CliError> {
    if cli.help_json {
        return Ok(Success::Schema(schema::schema()));
    }
    io::validate_stdin(&cli.images)?;
    let loaded = io::load_images(&cli.images)?;
    let spec = compose_loaded_images(&loaded, cli.compose_params())?;
    if cli.print_spec {
        return spec_json(&spec).map(Success::Spec);
    }
    io::validate_stdout(&cli.output, cli.format, cli.json)?;
    let images = loaded
        .into_iter()
        .map(|loaded| loaded.image)
        .collect::<Vec<_>>();
    let composed = render_spec(&spec, &images)?;
    io::export(cli.output, &composed, cli.format, cli.jpeg_quality).map(Success::Rendered)
}

fn json_requested(args: &[std::ffi::OsString]) -> bool {
    args.iter()
        .skip(1)
        .take_while(|argument| *argument != "--")
        .any(|argument| {
            argument == "--json"
                || argument
                    .to_str()
                    .is_some_and(|value| value.starts_with("--json="))
        })
}

fn parse(
    args: Vec<std::ffi::OsString>,
    json_output: bool,
) -> Result<Cli, Result<Success, CliError>> {
    Cli::try_parse_from(args).map_err(|error| {
        if !json_output {
            error.exit();
        }
        match error.kind() {
            clap::error::ErrorKind::DisplayHelp => Ok(Success::Help(error.to_string())),
            clap::error::ErrorKind::DisplayVersion => Ok(Success::Version),
            _ => Err(CliError::new("invalid_arguments", error.to_string())),
        }
    })
}

fn main() -> ExitCode {
    let args = std::env::args_os().collect::<Vec<_>>();
    let json_output = json_requested(&args);
    let outcome = match parse(args, json_output) {
        Ok(cli) => execute(cli),
        Err(outcome) => outcome,
    };
    match outcome {
        Ok(success) => {
            if json_output {
                println!("{}", success.json());
            } else if let Some(text) = success.human() {
                println!("{text}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => error.report(json_output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stitchr_core::{export_image, DynamicImage, ExportFormat};

    #[test]
    fn parses_documented_flags_and_requires_inputs() {
        let cli = Cli::try_parse_from([
            "stitchr",
            "one.png",
            "two.jpg",
            "--layout",
            "grid",
            "--spacing",
            "10",
        ])
        .unwrap();

        assert_eq!(
            cli.images,
            [PathBuf::from("one.png"), PathBuf::from("two.jpg")]
        );
        assert_eq!(cli.layout, CliLayout::Grid);
        assert_eq!(cli.spacing, 10);
        assert_eq!(cli.columns, 2);
        assert_eq!(cli.output, PathBuf::from("stitched.png"));
        assert!(Cli::try_parse_from(["stitchr"]).is_err());
        assert!(Cli::try_parse_from(["stitchr", "one.png", "--max-size", "100"]).is_err());
    }

    #[test]
    fn help_lists_options_defaults_and_examples() {
        let help = Cli::try_parse_from(["stitchr", "--help"])
            .unwrap_err()
            .to_string();

        assert!(help.contains("[possible values: grid, horizontal, vertical, freeform, split]"));
        assert!(help.contains("[default: stitched.png]"));
        assert!(help.contains("--position <X> <Y>"));
        assert!(help.contains("Examples:"));
    }

    #[test]
    fn schema_lists_flags_types_enums_defaults_and_alias() {
        assert!(
            Cli::try_parse_from(["stitchr", "--help-json"])
                .unwrap()
                .help_json
        );
        assert!(
            Cli::try_parse_from(["stitchr", "--schema"])
                .unwrap()
                .help_json
        );

        let value: Value = serde_json::from_str(&schema::schema().to_string()).unwrap();
        let args = value["arguments"].as_array().unwrap();
        let by_name = |name| args.iter().find(|arg| arg["name"] == name).unwrap();

        assert_eq!(by_name("images")["type"], "string");
        assert_eq!(by_name("images")["format"], "path");
        assert_eq!(by_name("images")["required"], true);
        assert_eq!(
            by_name("layout")["enum"],
            json!(["grid", "horizontal", "vertical", "freeform", "split"])
        );
        assert_eq!(by_name("layout")["default"], "grid");
        assert_eq!(by_name("divider")["type"], "integer");
        assert_eq!(by_name("divider")["default"], 0);
        assert_eq!(by_name("spacing")["type"], "integer");
        assert_eq!(by_name("spacing")["default"], 0);
        assert_eq!(by_name("background_color")["type"], "string");
        assert_eq!(by_name("background_color")["format"], "color");
        assert_eq!(by_name("background_color")["default"], "#000000");
        assert_eq!(by_name("label")["type"], "string");
        assert_eq!(by_name("label")["repeatable"], true);
        assert_eq!(by_name("border_color")["format"], "color");
        assert_eq!(by_name("border_width")["type"], "integer");
        assert_eq!(by_name("background_gradient")["format"], "color");
        assert_eq!(
            by_name("background_gradient")["value_names"],
            json!(["FROM", "TO"])
        );
        assert_eq!(
            by_name("gradient_kind")["enum"],
            json!(["linear", "radial"])
        );
        assert_eq!(by_name("gradient_angle")["default"], 180);
        assert_eq!(by_name("jpeg_quality")["type"], "integer");
        assert_eq!(by_name("jpeg_quality")["default"], 75);
        assert_eq!(by_name("format")["enum"], json!(["png", "jpeg"]));
        assert!(by_name("format")["default"].is_null());
        assert!(Cli::try_parse_from(["stitchr", "image.png", "--jpeg-quality", "0"]).is_err());
        assert!(Cli::try_parse_from(["stitchr", "image.png", "--jpeg-quality", "101"]).is_err());
        assert!(Cli::try_parse_from([
            "stitchr",
            "image.png",
            "--background",
            "red",
            "--background-gradient",
            "red",
            "blue",
        ])
        .is_err());
        assert!(args.iter().all(|arg| arg["name"] != "max_size"));
        assert_eq!(by_name("position")["repeatable"], true);
        assert_eq!(by_name("position")["values_per_occurrence"]["min"], 2);
        assert_eq!(by_name("help_json")["type"], "boolean");
        assert_eq!(by_name("help_json")["default"], false);
        assert_eq!(by_name("help_json")["aliases"], json!(["--schema"]));
        assert_eq!(by_name("print_spec")["type"], "boolean");
        assert!(by_name("help_json")["value_names"].is_null());
        assert_eq!(
            by_name("images")["required_unless"],
            json!(["--help-json", "--schema"])
        );
        assert!(value["examples"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn non_negative_accepts_zero_and_u32_range() {
        assert_eq!(non_negative("0"), Ok(0));
        assert_eq!(non_negative("42"), Ok(42));
        assert_eq!(non_negative("+7"), Ok(7));
        assert_eq!(non_negative("4294967295"), Ok(u32::MAX));
    }

    #[test]
    fn non_negative_rejects_negative_numbers_with_a_bound_message() {
        assert_eq!(non_negative("-5"), Err("must be ≥ 0".to_owned()));
        assert_eq!(non_negative("-1"), Err("must be ≥ 0".to_owned()));
    }

    #[test]
    fn non_negative_rejects_numbers_above_u32_max() {
        assert_eq!(
            non_negative("4294967296"),
            Err("must be at most 4294967295".to_owned())
        );
    }

    #[test]
    fn non_negative_rejects_non_integers() {
        for value in ["", "abc", "1.5", "-", "-x", "1e3"] {
            assert_eq!(
                non_negative(value),
                Err("must be an integer ≥ 0".to_owned()),
                "{value:?}"
            );
        }
    }

    fn parse_error(args: &[&str]) -> String {
        Cli::try_parse_from(std::iter::once("stitchr").chain(args.iter().copied()))
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn negative_pixel_options_report_the_bound_instead_of_a_double_dash_hint() {
        for option in ["--spacing", "--divider", "--border-width"] {
            let message = parse_error(&["image.png", option, "-5"]);

            assert!(
                message.contains("invalid value '-5'"),
                "{option}: {message}"
            );
            assert!(message.contains("must be ≥ 0"), "{option}: {message}");
            assert!(!message.contains("use '-- -5'"), "{option}: {message}");
        }
    }

    #[test]
    fn negative_freeform_coordinates_report_the_bound() {
        let message = parse_error(&["image.png", "--layout", "freeform", "--position", "3", "-4"]);

        assert!(message.contains("invalid value '-4'"), "{message}");
        assert!(message.contains("must be ≥ 0"), "{message}");
    }

    #[test]
    fn negative_ranged_options_report_their_range() {
        for (option, range) in [
            ("--columns", "1.."),
            ("--label-size", "1.."),
            ("--jpeg-quality", "1..=100"),
            ("--gradient-angle", "0..360"),
        ] {
            let message = parse_error(&["image.png", option, "-1"]);

            assert!(
                message.contains("invalid value '-1'"),
                "{option}: {message}"
            );
            assert!(message.contains(range), "{option}: {message}");
            assert!(!message.contains("use '-- -1'"), "{option}: {message}");
        }
    }

    #[test]
    fn negative_number_is_still_an_input_path_after_double_dash() {
        let cli = Cli::try_parse_from(["stitchr", "--spacing", "3", "--", "-5"]).unwrap();

        assert_eq!(cli.images, [PathBuf::from("-5")]);
        assert_eq!(cli.spacing, 3);
    }

    #[test]
    fn repeated_background_gradient_is_rejected_instead_of_silently_dropped() {
        let message = parse_error(&[
            "image.png",
            "--background-gradient",
            "red",
            "blue",
            "--background-gradient",
            "green",
            "white",
        ]);

        assert!(message.contains("--background-gradient"), "{message}");
        assert!(
            message.contains("cannot be used multiple times"),
            "{message}"
        );
    }

    #[test]
    fn single_background_gradient_keeps_both_colors() {
        let cli = Cli::try_parse_from([
            "stitchr",
            "image.png",
            "--background-gradient",
            "red",
            "blue",
        ])
        .unwrap();

        assert_eq!(
            cli.background_gradient,
            [
                RgbColor {
                    red: 255,
                    green: 0,
                    blue: 0
                },
                RgbColor {
                    red: 0,
                    green: 0,
                    blue: 255
                }
            ]
        );
    }

    #[test]
    fn background_gradient_requires_exactly_two_colors() {
        assert!(
            Cli::try_parse_from(["stitchr", "image.png", "--background-gradient", "red"]).is_err()
        );
    }

    #[test]
    fn schema_marks_background_gradient_as_not_repeatable() {
        let value = schema::schema();
        let gradient = value["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|arg| arg["name"] == "background_gradient")
            .unwrap();

        assert_eq!(gradient["repeatable"], false);
        assert_eq!(
            gradient["values_per_occurrence"],
            json!({"min": 2, "max": 2})
        );
    }

    fn render(cli: Cli) -> io::ExportResult {
        let loaded = io::load_images(&cli.images).unwrap();
        let spec = compose_loaded_images(&loaded, cli.compose_params()).unwrap();
        let pixels = loaded
            .iter()
            .map(|image| image.image.clone())
            .collect::<Vec<_>>();
        let composed = render_spec(&spec, &pixels).unwrap();
        io::export(cli.output, &composed, cli.format, cli.jpeg_quality).unwrap()
    }

    #[test]
    fn creates_png_and_jpeg_from_two_images() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.png");
        let second = directory.path().join("second.jpg");
        let png = directory.path().join("combined.png");
        let jpeg = directory.path().join("combined.jpeg");

        export_image(&DynamicImage::new_rgba8(2, 1), &first, ExportFormat::Png).unwrap();
        export_image(&DynamicImage::new_rgb8(1, 1), &second, ExportFormat::Jpeg).unwrap();

        let freeform = Cli::try_parse_from([
            "stitchr",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
            "--layout",
            "freeform",
            "--position",
            "0",
            "0",
            "--position",
            "3",
            "2",
            "--output",
            png.to_str().unwrap(),
        ])
        .unwrap();
        assert_eq!(render(freeform).output, png);
        let result = stitchr_core::load_image(&png).unwrap();
        assert_eq!((result.width(), result.height()), (4, 3));

        let vertical = Cli::try_parse_from([
            "stitchr",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
            "--layout",
            "vertical",
            "--spacing",
            "2",
            "--output",
            jpeg.to_str().unwrap(),
        ])
        .unwrap();
        assert_eq!(render(vertical).output, jpeg);
        let result = stitchr_core::load_image(&jpeg).unwrap();
        assert_eq!((result.width(), result.height()), (6, 8));
        assert!(std::fs::read(&jpeg)
            .unwrap()
            .starts_with(&[0xff, 0xd8, 0xff]));
    }

    #[test]
    fn json_is_requested_only_by_a_bare_flag_before_the_separator() {
        let args = |values: &[&str]| {
            std::iter::once("stitchr")
                .chain(values.iter().copied())
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>()
        };

        assert!(json_requested(&args(&["a.png", "--json"])));
        assert!(json_requested(&args(&["--json=true", "a.png"])));
        assert!(!json_requested(&args(&["a.png"])));
        assert!(!json_requested(&args(&["--", "--json"])));
        assert!(!json_requested(&args(&["a.png", "--output=--json"])));
    }

    #[test]
    fn rendered_success_json_has_the_render_fields() {
        let success = Success::Rendered(io::ExportResult {
            output: PathBuf::from("out.png"),
            width: 3,
            height: 4,
            format: "png",
        });

        assert_eq!(
            success.json(),
            json!({
                "schema_version": SCHEMA_VERSION, "ok": true, "output": "out.png",
                "width": 3, "height": 4, "format": "png"
            })
        );
        assert_eq!(success.human().as_deref(), Some("out.png"));
    }

    #[test]
    fn stdout_render_prints_nothing_in_human_mode() {
        let success = Success::Rendered(io::ExportResult {
            output: PathBuf::from("-"),
            width: 1,
            height: 1,
            format: "png",
        });

        assert_eq!(success.human(), None);
    }

    #[test]
    fn query_successes_wrap_their_payload_under_one_key() {
        let payload = json!({ "a": 1 });

        assert_eq!(
            Success::Spec(payload.clone()).json(),
            json!({ "schema_version": SCHEMA_VERSION, "ok": true, "spec": payload })
        );
        assert_eq!(
            Success::Schema(payload.clone()).json(),
            json!({ "schema_version": SCHEMA_VERSION, "ok": true, "schema": payload })
        );
        assert_eq!(
            Success::Help("usage".into()).json(),
            json!({ "schema_version": SCHEMA_VERSION, "ok": true, "help": "usage" })
        );
        assert_eq!(
            Success::Version.json(),
            json!({
                "schema_version": SCHEMA_VERSION, "ok": true,
                "version": env!("CARGO_PKG_VERSION")
            })
        );
    }

    #[test]
    fn spec_query_human_output_is_pretty_printed() {
        let text = Success::Spec(json!({ "a": 1 })).human().unwrap();

        assert_eq!(text, "{\n  \"a\": 1\n}");
    }

    #[test]
    fn spec_json_stamps_the_contract_version() {
        let cli = Cli::try_parse_from(["stitchr", "a.png"]).unwrap();
        let spec = CliComposer::compose(
            &[stitchr_core::ImageMetadata {
                name: "a.png".into(),
                width: 2,
                height: 1,
            }],
            cli.compose_params(),
        )
        .unwrap();

        let value = spec_json(&spec).unwrap();

        assert_eq!(value["schema_version"], SCHEMA_VERSION);
        assert_eq!(value["width"], 2);
    }
}
