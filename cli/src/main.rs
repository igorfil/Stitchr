mod composer;
mod error;

use clap::{Arg, ArgAction, CommandFactory, Parser, ValueEnum, ValueHint};
use composer::{CliComposeParams, CliComposer};
use error::CliError;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};
use stitchr_core::{
    export_image_with_quality, load_image_from_bytes, load_image_with_metadata, render_spec,
    write_image, CompositionSpec, DynamicImage, ExportFormat, ImageMetadata, LoadedImage, RgbColor,
    DEFAULT_JPEG_QUALITY,
};

const SCHEMA_VERSION: u32 = 1;
const STDIO_PATH: &str = "-";

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

    #[arg(long, num_args = 2, value_names = ["FROM", "TO"], conflicts_with = "background_color", help = "Gradient background from one color to another, instead of --background; split dividers use FROM")]
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

fn argument_schema(arg: &Arg) -> Value {
    let choices: Vec<_> = arg
        .get_possible_values()
        .into_iter()
        .filter(|value| !value.is_hide_set())
        .map(|value| value.get_name().to_owned())
        .collect();
    let boolean = matches!(
        arg.get_action(),
        ArgAction::SetTrue | ArgAction::Help | ArgAction::Version
    );
    let path = arg.get_value_hint() == ValueHint::AnyPath;
    let color = matches!(
        arg.get_id().as_str(),
        "background_color" | "border_color" | "background_gradient"
    );
    let text = arg.get_id().as_str() == "label";
    let value_type = if boolean {
        "boolean"
    } else if path || color || text || !choices.is_empty() {
        "string"
    } else {
        "integer"
    };
    let default = if boolean {
        json!(false)
    } else {
        match arg.get_default_values().first() {
            Some(value) if value_type == "integer" => value
                .to_string_lossy()
                .parse::<u64>()
                .map_or(Value::Null, |number| json!(number)),
            Some(value) => json!(value.to_string_lossy()),
            None => Value::Null,
        }
    };
    let arity = arg.get_num_args();

    json!({
        "name": arg.get_id().as_str(),
        "long": arg.get_long().map(|long| format!("--{long}")),
        "short": arg.get_short().map(|short| format!("-{short}")),
        "aliases": arg.get_visible_aliases().unwrap_or_default().into_iter().map(|alias| format!("--{alias}")).collect::<Vec<_>>(),
        "kind": if arg.is_positional() { "positional" } else { "option" },
        "type": value_type,
        "format": if path { Some("path") } else if color { Some("color") } else { None },
        "enum": choices,
        "default": default,
        "required": arg.is_required_set() || arg.is_positional(),
        "required_unless": if arg.is_positional() { vec!["--help-json", "--schema"] } else { vec![] },
        "repeatable": matches!(arg.get_action(), ArgAction::Append),
        "values_per_occurrence": {
            "min": arity.map(|range| range.min_values()).unwrap_or(0),
            "max": arity.and_then(|range| (range.max_values() != usize::MAX).then_some(range.max_values()))
        },
        "value_names": if boolean { None } else { arg.get_value_names().map(|names| names.iter().map(ToString::to_string).collect::<Vec<_>>()) },
        "help": arg.get_help().map(ToString::to_string)
    })
}

fn schema() -> Value {
    let mut command = Cli::command();
    command.build();
    let usage = command.render_usage().to_string();
    let after_help = command
        .get_after_help()
        .map(|text| text.to_string())
        .unwrap_or_default();
    let examples = after_help
        .lines()
        .skip(1)
        .map(str::trim)
        .collect::<Vec<_>>();
    let arguments = command
        .get_arguments()
        .map(argument_schema)
        .collect::<Vec<_>>();

    json!({
        "schema_version": SCHEMA_VERSION,
        "name": command.get_name(),
        "version": command.get_version(),
        "description": command.get_long_about().map(ToString::to_string),
        "usage": usage,
        "examples": examples,
        "arguments": arguments
    })
}

fn is_stdio(path: &Path) -> bool {
    path.as_os_str() == STDIO_PATH
}

fn output_format(output: &Path, format: Option<CliFormat>) -> Result<ExportFormat, CliError> {
    match format {
        Some(CliFormat::Png) => return Ok(ExportFormat::Png),
        Some(CliFormat::Jpeg) => return Ok(ExportFormat::Jpeg),
        None if is_stdio(output) => {
            return Err(CliError::new(
                "unsupported_output_format",
                "-o - requires --format png or --format jpeg",
            ))
        }
        None => {}
    }
    match output.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("png") => Ok(ExportFormat::Png),
        Some(extension)
            if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") =>
        {
            Ok(ExportFormat::Jpeg)
        }
        _ => Err(CliError::new(
            "unsupported_output_format",
            "output filename must end in .png, .jpg, or .jpeg",
        )),
    }
}

fn load_stdin() -> Result<LoadedImage, CliError> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .read_to_end(&mut bytes)
        .map_err(|error| CliError::new("input_error", format!("cannot read stdin: {error}")))?;
    let image = load_image_from_bytes(&bytes)
        .map_err(|error| CliError::new("input_error", format!("cannot load stdin: {error}")))?;
    let metadata = ImageMetadata {
        name: STDIO_PATH.into(),
        width: image.width(),
        height: image.height(),
    };
    Ok(LoadedImage { image, metadata })
}

fn load_images(paths: &[PathBuf]) -> Result<Vec<LoadedImage>, CliError> {
    paths
        .iter()
        .map(|path| {
            if is_stdio(path) {
                return load_stdin();
            }
            load_image_with_metadata(path).map_err(|error| {
                CliError::new(
                    "input_error",
                    format!("cannot load {}: {error}", path.display()),
                )
            })
        })
        .collect()
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

struct ExportResult {
    output: PathBuf,
    width: u32,
    height: u32,
    format: &'static str,
}

fn validate_stdin(images: &[PathBuf]) -> Result<(), CliError> {
    if images.iter().filter(|path| is_stdio(path)).count() > 1 {
        return Err(CliError::new(
            "invalid_arguments",
            "- (stdin) can be used as an input only once",
        ));
    }
    Ok(())
}

fn validate_stdout(output: &Path, format: Option<CliFormat>, json: bool) -> Result<(), CliError> {
    if !is_stdio(output) {
        return Ok(());
    }
    if json {
        return Err(CliError::new(
            "invalid_arguments",
            "-o - cannot be combined with --json because stdout carries the image",
        ));
    }
    if format.is_none() {
        return Err(CliError::new(
            "invalid_arguments",
            "-o - requires --format png or --format jpeg",
        ));
    }
    Ok(())
}

fn write_output(
    output: &Path,
    composed: &DynamicImage,
    format: ExportFormat,
    jpeg_quality: u8,
) -> Result<(), CliError> {
    if !is_stdio(output) {
        return export_image_with_quality(composed, output, format, jpeg_quality).map_err(
            |error| {
                CliError::new(
                    "output_error",
                    format!("cannot write {}: {error}", output.display()),
                )
            },
        );
    }
    // Encode fully before writing so an encoder failure never leaves a truncated image on stdout.
    let mut bytes = Vec::new();
    write_image(composed, &mut bytes, format, jpeg_quality)
        .map_err(|error| CliError::new("output_error", format!("cannot encode image: {error}")))?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&bytes)
        .and_then(|()| stdout.flush())
        .map_err(|error| CliError::new("output_error", format!("cannot write stdout: {error}")))
}

fn run(
    output: PathBuf,
    composed: DynamicImage,
    format: Option<CliFormat>,
    jpeg_quality: u8,
) -> Result<ExportResult, CliError> {
    let format = output_format(&output, format)?;
    write_output(&output, &composed, format, jpeg_quality)?;
    Ok(ExportResult {
        output,
        width: composed.width(),
        height: composed.height(),
        format: match format {
            ExportFormat::Png => "png",
            ExportFormat::Jpeg => "jpeg",
        },
    })
}

fn main() -> ExitCode {
    let args = std::env::args_os().collect::<Vec<_>>();
    let json_output = args
        .iter()
        .skip(1)
        .take_while(|argument| *argument != "--")
        .any(|argument| {
            argument == "--json"
                || argument
                    .to_str()
                    .is_some_and(|value| value.starts_with("--json="))
        });
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            if !json_output {
                error.exit();
            }
            match error.kind() {
                clap::error::ErrorKind::DisplayHelp => {
                    println!(
                        "{}",
                        json!({
                            "schema_version": SCHEMA_VERSION, "ok": true, "help": error.to_string()
                        })
                    );
                    return ExitCode::SUCCESS;
                }
                clap::error::ErrorKind::DisplayVersion => {
                    println!(
                        "{}",
                        json!({
                            "schema_version": SCHEMA_VERSION, "ok": true, "version": env!("CARGO_PKG_VERSION")
                        })
                    );
                    return ExitCode::SUCCESS;
                }
                _ => {
                    return CliError::new("invalid_arguments", error.to_string())
                        .report(true, ExitCode::from(2))
                }
            }
        }
    };
    if cli.help_json {
        let schema = schema();
        if cli.json {
            println!(
                "{}",
                json!({
                    "schema_version": SCHEMA_VERSION, "ok": true, "schema": schema
                })
            );
        } else {
            println!("{schema}");
        }
        return ExitCode::SUCCESS;
    }

    if let Err(error) = validate_stdin(&cli.images) {
        return error.report(cli.json, ExitCode::from(2));
    }
    let loaded = match load_images(&cli.images) {
        Ok(images) => images,
        Err(error) => {
            return error.report(cli.json, ExitCode::FAILURE);
        }
    };
    let spec = match compose_loaded_images(
        &loaded,
        CliComposeParams {
            layout: cli.layout,
            columns: cli.columns,
            spacing: cli.spacing,
            alignment: cli.alignment,
            background_color: cli.background_color,
            positions: &cli.position,
            divider: cli.divider,
            labels: &cli.label,
            border_width: cli.border_width,
            border_color: cli.border_color,
            label_size: cli.label_size,
            background_gradient: &cli.background_gradient,
            gradient_kind: cli.gradient_kind,
            gradient_angle: cli.gradient_angle,
        },
    ) {
        Ok(spec) => spec,
        Err(error) => {
            return error.report(cli.json, ExitCode::FAILURE);
        }
    };
    if cli.print_spec {
        return match serde_json::to_value(&spec) {
            Ok(mut spec) => {
                spec["schema_version"] = json!(SCHEMA_VERSION);
                if cli.json {
                    println!(
                        "{}",
                        json!({
                            "schema_version": SCHEMA_VERSION, "ok": true, "spec": spec
                        })
                    );
                } else {
                    println!("{spec:#}");
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                CliError::new("serialization_error", format!("cannot print spec: {error}"))
                    .report(cli.json, ExitCode::FAILURE)
            }
        };
    }

    if let Err(error) = validate_stdout(&cli.output, cli.format, cli.json) {
        return error.report(cli.json, ExitCode::from(2));
    }
    let images = loaded
        .into_iter()
        .map(|loaded| loaded.image)
        .collect::<Vec<_>>();
    let composed = match render_spec(&spec, &images) {
        Ok(image) => image,
        Err(error) => {
            return CliError::from(error).report(cli.json, ExitCode::FAILURE);
        }
    };
    match run(cli.output, composed, cli.format, cli.jpeg_quality) {
        Ok(result) => {
            if cli.json {
                println!(
                    "{}",
                    json!({
                        "schema_version": SCHEMA_VERSION,
                        "ok": true,
                        "output": result.output.to_string_lossy(),
                        "width": result.width,
                        "height": result.height,
                        "format": result.format
                    })
                );
            } else if !is_stdio(&result.output) {
                println!("{}", result.output.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => error.report(cli.json, ExitCode::FAILURE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stitchr_core::{export_image, DynamicImage};

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

        let value: Value = serde_json::from_str(&schema().to_string()).unwrap();
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
        let loaded = load_images(&freeform.images).unwrap();
        let spec = compose_loaded_images(
            &loaded,
            CliComposeParams {
                layout: freeform.layout,
                columns: freeform.columns,
                spacing: freeform.spacing,
                alignment: freeform.alignment,
                background_color: freeform.background_color,
                positions: &freeform.position,
                divider: freeform.divider,
                labels: &freeform.label,
                border_width: freeform.border_width,
                border_color: freeform.border_color,
                label_size: freeform.label_size,
                background_gradient: &freeform.background_gradient,
                gradient_kind: freeform.gradient_kind,
                gradient_angle: freeform.gradient_angle,
            },
        )
        .unwrap();
        let pixels = loaded
            .iter()
            .map(|image| image.image.clone())
            .collect::<Vec<_>>();
        let composed = render_spec(&spec, &pixels).unwrap();
        assert_eq!(
            run(
                freeform.output.clone(),
                composed,
                freeform.format,
                freeform.jpeg_quality
            )
            .unwrap()
            .output,
            png
        );
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
        let loaded = load_images(&vertical.images).unwrap();
        let spec = compose_loaded_images(
            &loaded,
            CliComposeParams {
                layout: vertical.layout,
                columns: vertical.columns,
                spacing: vertical.spacing,
                alignment: vertical.alignment,
                background_color: vertical.background_color,
                positions: &vertical.position,
                divider: vertical.divider,
                labels: &vertical.label,
                border_width: vertical.border_width,
                border_color: vertical.border_color,
                label_size: vertical.label_size,
                background_gradient: &vertical.background_gradient,
                gradient_kind: vertical.gradient_kind,
                gradient_angle: vertical.gradient_angle,
            },
        )
        .unwrap();
        let pixels = loaded
            .iter()
            .map(|image| image.image.clone())
            .collect::<Vec<_>>();
        let composed = render_spec(&spec, &pixels).unwrap();
        assert_eq!(
            run(
                vertical.output.clone(),
                composed,
                vertical.format,
                vertical.jpeg_quality
            )
            .unwrap()
            .output,
            jpeg
        );
        let result = stitchr_core::load_image(&jpeg).unwrap();
        assert_eq!((result.width(), result.height()), (6, 8));
        assert!(std::fs::read(&jpeg)
            .unwrap()
            .starts_with(&[0xff, 0xd8, 0xff]));
    }
}
