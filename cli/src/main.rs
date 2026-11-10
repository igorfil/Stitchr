mod composer;
mod error;
mod io;
mod options;
mod schema;
mod spec_file;
mod warnings;

use clap::{parser::ValueSource, ArgAction, CommandFactory, FromArgMatches, Parser, ValueEnum};
use composer::{CliComposeParams, CliComposer, LoadedFont};
use error::CliError;
use io::is_stdio;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::ExitCode,
};
use stitchr_core::{
    fit, label_warnings, read_font, render_spec, ComposeError, CompositionSpec, DynamicImage,
    FitTarget, ImageMetadata, LabelPosition, LoadedImage, PlacementFit, RgbColor,
    DEFAULT_JPEG_QUALITY,
};
use warnings::Warning;

const SCHEMA_VERSION: u32 = 1;
const DEFAULT_SCHEMA_FORMAT: &str = "stitchr";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliFormat {
    Png,
    #[value(alias = "jpg")]
    Jpeg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum SchemaFormat {
    Stitchr,
    Jsonschema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliLabelPosition {
    Inside,
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliFit {
    Cover,
    Contain,
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
    about = "Combine PNG, JPEG, WebP, and GIF images into one PNG or JPEG image",
    long_about = "Arrange PNG, JPEG, WebP, or GIF images in a grid, horizontal or vertical stack, at explicit freeform positions, or as diagonal split slices of same-size images. Output format is selected by the output file extension.",
    after_help = "Examples:\n  stitchr img1.png img2.png --layout grid --spacing 10\n  stitchr img1.png img2.jpg --layout horizontal --alignment center -o combined.jpg\n  stitchr img1.png img2.png --layout freeform --position 0 0 --position 300 80 -o combined.png\n  stitchr light.png dark.png --layout split --divider 4 --background white -o split.png\n  stitchr before.png after.png --layout horizontal --spacing 16 --background white --label Before --label After -o labeled.png\n  stitchr img1.png img2.png --spacing 40 --background-gradient '#ff7e5f' '#feb47b' --gradient-angle 135 -o gradient.png\n  stitchr img1.png img2.png --background rebeccapurple --print-spec"
)]
struct Cli {
    #[arg(required_unless_present_any = ["help_json", "spec"], num_args = 1.., value_name = "IMAGE", help = "Input PNG, JPEG, WebP, or GIF paths (format detected from content; GIF uses its first frame), or - to read one image from stdin (one or more; required unless --help-json/--schema or --spec)")]
    images: Vec<PathBuf>,

    #[arg(
        long,
        value_name = "FILE",
        conflicts_with = "images",
        help = "Render a spec printed by --print-spec (file, or - for stdin) instead of composing inputs; layout options are ignored"
    )]
    spec: Option<PathBuf>,

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
        value_enum,
        default_value = "inside",
        help = "Label tab placement: inside covers the image's top-left corner, outside adds a header strip above the image (only with --label)"
    )]
    label_position: CliLabelPosition,

    #[arg(
        long,
        value_name = "COLOR",
        help = "Label text color: RGB #RRGGBB, rgb(r,g,b), or common color name; default: black or white for contrast with --border-color (only with --label)"
    )]
    label_color: Option<RgbColor>,

    #[arg(
        long,
        value_name = "FILE",
        help = "TrueType/OpenType font file for labels, for example to draw scripts the bundled Inter font lacks (only with --label)"
    )]
    font: Option<PathBuf>,

    #[arg(long, value_name = "PIXELS|min", allow_negative_numbers = true, value_parser = fit_target, help = "Shrink taller images to this height, keeping aspect ratio; min matches the shortest input. Images are never enlarged (positive integer or min; not for split)")]
    fit_height: Option<FitTarget>,

    #[arg(long, value_name = "PIXELS|min", allow_negative_numbers = true, value_parser = fit_target, help = "Shrink wider images to this width, keeping aspect ratio; min matches the narrowest input. Images are never enlarged (positive integer or min; not for split)")]
    fit_width: Option<FitTarget>,

    #[arg(
        long,
        value_enum,
        value_name = "MODE",
        help = "Fit split inputs of different sizes into the smallest input width and height: cover crops, contain pads (only for split)"
    )]
    fit: Option<CliFit>,

    #[arg(long, allow_negative_numbers = true, value_parser = clap::value_parser!(u32).range(1..), value_name = "PIXELS", help = "Shrink the images so the whole output is at most this wide; spacing, borders, and labels keep their size (positive integer)")]
    max_width: Option<u32>,

    #[arg(long, allow_negative_numbers = true, value_parser = clap::value_parser!(u32).range(1..), value_name = "PIXELS", help = "Shrink the images so the whole output is at most this tall; spacing, borders, and labels keep their size (positive integer)")]
    max_height: Option<u32>,
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

    #[arg(long = "help-json", visible_alias = "schema", value_enum, num_args = 0..=1, require_equals = true, default_missing_value = DEFAULT_SCHEMA_FORMAT, value_name = "FORMAT", help = "Print the full CLI specification as JSON: stitchr (default) or jsonschema for a standard JSON Schema of the arguments, e.g. --schema=jsonschema")]
    help_json: Option<SchemaFormat>,

    #[arg(long = "print-spec", action = ArgAction::SetTrue, help = "Print the composition spec as JSON after loading inputs (boolean; default: false)")]
    print_spec: bool,

    #[arg(long, action = ArgAction::SetTrue, help = "Print versioned JSON results and errors to stdout")]
    json: bool,

    #[arg(long, action = ArgAction::SetTrue, help = "Allow the output to overwrite one of the inputs (boolean; default: false)")]
    force: bool,

    #[arg(long, action = ArgAction::SetTrue, help = "Fail with output_exists instead of overwriting an existing output file (boolean; default: false)")]
    no_clobber: bool,

    #[arg(long, action = ArgAction::SetTrue, help = "Create missing parent directories of the output file (boolean; default: false)")]
    mkdir: bool,

    #[arg(long, action = ArgAction::SetTrue, help = "Fail with invalid_arguments instead of rendering when there are warnings (boolean; default: false)")]
    strict: bool,

    #[arg(skip)]
    explicit: Vec<String>,
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

fn fit_target(value: &str) -> Result<FitTarget, String> {
    if value == "min" {
        return Ok(FitTarget::Min);
    }
    match non_negative(value) {
        Ok(0) | Err(_) => Err("must be a positive integer or min".to_owned()),
        Ok(pixels) => Ok(FitTarget::Pixels(pixels)),
    }
}

impl Cli {
    fn destination(&self) -> io::Destination {
        io::Destination {
            path: self.output.clone(),
            format: self.format,
            jpeg_quality: self.jpeg_quality,
            no_clobber: self.no_clobber,
            mkdir: self.mkdir,
        }
    }

    fn compose_params<'a>(&'a self, font: Option<&'a LoadedFont>) -> CliComposeParams<'a> {
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
            label_position: match self.label_position {
                CliLabelPosition::Inside => LabelPosition::Inside,
                CliLabelPosition::Outside => LabelPosition::Outside,
            },
            label_color: self.label_color,
            font,
        }
    }
}

#[derive(Debug)]
enum Success {
    Help(String),
    Version,
    Schema(Value),
    Spec {
        spec: Value,
        warnings: Vec<Warning>,
    },
    Rendered {
        result: io::ExportResult,
        scale: f64,
        warnings: Vec<Warning>,
    },
}

impl Success {
    fn json(&self) -> Value {
        let mut response = match self {
            Self::Help(help) => json!({ "help": help }),
            Self::Version => json!({ "version": env!("CARGO_PKG_VERSION") }),
            Self::Schema(schema) => json!({ "schema": schema }),
            Self::Spec { spec, .. } => json!({ "spec": spec }),
            Self::Rendered { result, scale, .. } => json!({
                "output": result.output.to_string_lossy(),
                "output_abs": result.output_abs.as_ref().map(|path| path.to_string_lossy()),
                "width": result.width,
                "height": result.height,
                "format": result.format,
                "scale": scale
            }),
        };
        if let Self::Spec { warnings, .. } | Self::Rendered { warnings, .. } = self {
            response["warnings"] = warnings.iter().map(Warning::json).collect();
        }
        response["schema_version"] = json!(SCHEMA_VERSION);
        response["ok"] = json!(true);
        response
    }

    fn human(&self) -> Option<String> {
        match self {
            Self::Help(help) => Some(help.clone()),
            Self::Version => Some(format!("stitchr {}", env!("CARGO_PKG_VERSION"))),
            Self::Schema(schema) => Some(schema.to_string()),
            Self::Spec { spec, .. } => Some(format!("{spec:#}")),
            Self::Rendered { result, .. } if is_stdio(&result.output) => None,
            Self::Rendered { result, .. } => Some(result.output.display().to_string()),
        }
    }

    fn warnings(&self) -> &[Warning] {
        match self {
            Self::Spec { warnings, .. } | Self::Rendered { warnings, .. } => warnings,
            Self::Help(_) | Self::Version | Self::Schema(_) => &[],
        }
    }
}

fn spec_json(spec: &CompositionSpec) -> Result<Value, CliError> {
    let mut value = serde_json::to_value(spec).map_err(|error| {
        CliError::new("serialization_error", format!("cannot print spec: {error}"))
    })?;
    value["schema_version"] = json!(SCHEMA_VERSION);
    Ok(value)
}

fn collect_warnings(
    cli: &Cli,
    spec: &CompositionSpec,
    metadata: &[ImageMetadata],
    scale: f64,
) -> Vec<Warning> {
    let labels = spec
        .frame
        .as_ref()
        .map_or(&[][..], |frame| frame.labels.as_slice());
    let mut collected = warnings::option_warnings(cli);
    collected.extend(warnings::format_warnings(cli));
    collected.extend(warnings::fit_warnings(cli, metadata));
    collected.extend(warnings::scale_warnings(spec, metadata, scale));
    collected.extend(warnings::label_warnings(&label_warnings(spec), labels));
    collected
}

struct Request {
    inputs: Vec<PathBuf>,
    loaded: Vec<LoadedImage>,
    metadata: Vec<ImageMetadata>,
    spec: CompositionSpec,
    scale: f64,
}

fn resolved_path(path: &Path) -> String {
    if is_stdio(path) {
        return path.to_string_lossy().into_owned();
    }
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn metadata_of(loaded: &[LoadedImage]) -> Vec<ImageMetadata> {
    loaded.iter().map(|image| image.metadata.clone()).collect()
}

/// Sizes the images are drawn at before any `--max-width`/`--max-height` scaling.
fn fitted_sizes(cli: &Cli, metadata: &[ImageMetadata]) -> Vec<(u32, u32)> {
    let sizes = metadata
        .iter()
        .map(|image| (image.width, image.height))
        .collect::<Vec<_>>();
    match (cli.layout, cli.fit) {
        (CliLayout::Split, Some(_)) => vec![fit::split_target(&sizes); sizes.len()],
        (CliLayout::Split, None) => sizes,
        _ => fit::fit_sizes(&sizes, cli.fit_height, cli.fit_width),
    }
}

fn compose_scaled(
    cli: &Cli,
    font: Option<&LoadedFont>,
    metadata: &[ImageMetadata],
    sizes: &[(u32, u32)],
    scale: f64,
) -> Result<CompositionSpec, ComposeError> {
    let scaled = metadata
        .iter()
        .zip(sizes)
        .map(|(image, &size)| {
            let (width, height) = fit::scale_size(size, scale);
            ImageMetadata {
                name: image.name.clone(),
                width,
                height,
            }
        })
        .collect::<Vec<_>>();
    let positions = cli
        .position
        .iter()
        .map(|&coordinate| (f64::from(coordinate) * scale).floor() as u32)
        .collect::<Vec<_>>();
    CliComposer::compose(
        &scaled,
        CliComposeParams {
            positions: &positions,
            ..cli.compose_params(font)
        },
    )
}

fn request_from_arguments(cli: &Cli) -> Result<Request, CliError> {
    io::validate_stdin(&cli.images)?;
    let loaded = io::load_images(&cli.images)?;
    let metadata = metadata_of(&loaded);
    let font = cli.font.as_deref().map(load_font).transpose()?;
    let sizes = fitted_sizes(cli, &metadata);
    let (mut spec, scale) = fit::fit_within((cli.max_width, cli.max_height), |scale| {
        compose_scaled(cli, font.as_ref(), &metadata, &sizes, scale)
    })
    .map_err(|error| CliError::from(error).with_input_paths(&cli.images))?;
    let placement_fit = match (cli.layout, cli.fit) {
        (CliLayout::Split, Some(CliFit::Cover)) => Some(PlacementFit::Cover),
        (CliLayout::Split, Some(CliFit::Contain)) => Some(PlacementFit::Contain),
        _ => None,
    };
    spec.images
        .iter_mut()
        .zip(&cli.images)
        .for_each(|(placement, path)| {
            placement.path = Some(resolved_path(path));
            placement.fit = placement_fit;
        });
    Ok(Request {
        inputs: cli.images.clone(),
        loaded,
        metadata,
        spec,
        scale,
    })
}

fn load_font(path: &Path) -> Result<LoadedFont, CliError> {
    read_font(path)
        .map(|data| LoadedFont {
            path: resolved_path(path),
            data,
        })
        .map_err(|error| {
            CliError::new(
                "input_error",
                format!("cannot load font {}: {error}", path.display()),
            )
            .with_details(json!({ "path": path.to_string_lossy(), "kind": "font" }))
        })
}

fn request_from_spec(source: &Path) -> Result<Request, CliError> {
    let mut spec = spec_file::read(source)?;
    if let Some(frame) = spec.frame.as_mut() {
        if let Some(path) = frame.font.clone() {
            frame.font_data = Some(load_font(Path::new(&path))?.data);
        }
    }
    let inputs = spec_file::input_paths(&spec);
    io::validate_stdin(&inputs)?;
    let loaded = io::load_images(&inputs)?;
    let metadata = metadata_of(&loaded);
    spec_file::check_sizes(&spec, &metadata)?;
    Ok(Request {
        inputs,
        loaded,
        metadata,
        spec,
        scale: 1.0,
    })
}

/// Shrinks or fits each image to the size its placement expects.
fn prepare_images(
    loaded: Vec<LoadedImage>,
    spec: &CompositionSpec,
    inputs: &[PathBuf],
) -> Result<Vec<DynamicImage>, CliError> {
    loaded
        .into_iter()
        .zip(&spec.images)
        .enumerate()
        .map(|(index, (loaded, placement))| {
            let image = loaded.image;
            if (image.width(), image.height()) == (placement.width, placement.height) {
                return Ok(image);
            }
            fit::prepare_image(index, &image, placement)
                .map(Cow::into_owned)
                .map_err(|error| CliError::from(error).with_input_paths(inputs))
        })
        .collect()
}

fn execute(cli: Cli) -> Result<Success, CliError> {
    match cli.help_json {
        Some(SchemaFormat::Stitchr) => return Ok(Success::Schema(schema::schema())),
        Some(SchemaFormat::Jsonschema) => return Ok(Success::Schema(schema::json_schema())),
        None => {}
    }
    let request = match &cli.spec {
        Some(source) => request_from_spec(source)?,
        None => request_from_arguments(&cli)?,
    };
    let warnings = collect_warnings(&cli, &request.spec, &request.metadata, request.scale);
    if cli.strict && !warnings.is_empty() {
        return Err(CliError::strict(&warnings));
    }
    if cli.print_spec {
        let mut spec = spec_json(&request.spec)?;
        if request.scale < 1.0 {
            spec["scale"] = json!(request.scale);
        }
        return Ok(Success::Spec { spec, warnings });
    }
    io::validate_stdout(&cli.output, cli.format, cli.json)?;
    io::validate_destination(&cli.output, &request.inputs, cli.force, cli.no_clobber)?;
    let images = prepare_images(request.loaded, &request.spec, &request.inputs)?;
    let composed = render_spec(&request.spec, &images)
        .map_err(|error| CliError::from(error).with_input_paths(&request.inputs))?;
    let result = io::export(cli.destination(), &composed)?;
    Ok(Success::Rendered {
        result,
        scale: request.scale,
        warnings,
    })
}

fn json_requested(args: &[OsString]) -> bool {
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

fn parse_cli(args: Vec<OsString>) -> Result<Cli, clap::Error> {
    let command = Cli::command();
    let ids = command
        .get_arguments()
        .map(|arg| arg.get_id().to_string())
        .collect::<Vec<_>>();
    let matches = command.try_get_matches_from(args)?;
    let mut cli = Cli::from_arg_matches(&matches)?;
    cli.explicit = ids
        .into_iter()
        .filter(|id| matches.value_source(id) == Some(ValueSource::CommandLine))
        .collect();
    Ok(cli)
}

fn parse(args: Vec<OsString>, json_output: bool) -> Result<Cli, Result<Success, CliError>> {
    parse_cli(args).map_err(|error| {
        if !json_output {
            error.exit();
        }
        match error.kind() {
            clap::error::ErrorKind::DisplayHelp => Ok(Success::Help(error.to_string())),
            clap::error::ErrorKind::DisplayVersion => Ok(Success::Version),
            _ => Err(CliError::from(&error)),
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
            } else {
                success
                    .warnings()
                    .iter()
                    .for_each(|warning| eprintln!("stitchr: warning: {}", warning.reason));
                if let Some(text) = success.human() {
                    println!("{text}");
                }
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
        assert_eq!(
            Cli::try_parse_from(["stitchr", "--help-json"])
                .unwrap()
                .help_json,
            Some(SchemaFormat::Stitchr)
        );
        assert_eq!(
            Cli::try_parse_from(["stitchr", "--schema"])
                .unwrap()
                .help_json,
            Some(SchemaFormat::Stitchr)
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
        assert_eq!(by_name("help_json")["type"], "string");
        assert_eq!(
            by_name("help_json")["enum"],
            json!(["stitchr", "jsonschema"])
        );
        assert!(by_name("help_json")["default"].is_null());
        assert_eq!(by_name("help_json")["default_missing_value"], "stitchr");
        assert_eq!(by_name("help_json")["require_equals"], true);
        assert_eq!(by_name("help_json")["aliases"], json!(["--schema"]));
        assert_eq!(by_name("print_spec")["type"], "boolean");
        assert!(by_name("print_spec")["value_names"].is_null());
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
        let metadata = loaded
            .iter()
            .map(|image| image.metadata.clone())
            .collect::<Vec<_>>();
        let spec = CliComposer::compose(&metadata, cli.compose_params(None)).unwrap();
        let pixels = loaded
            .iter()
            .map(|image| image.image.clone())
            .collect::<Vec<_>>();
        let composed = render_spec(&spec, &pixels).unwrap();
        io::export(cli.destination(), &composed).unwrap()
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
        let success = Success::Rendered {
            result: io::ExportResult {
                output: PathBuf::from("out.png"),
                output_abs: Some(PathBuf::from("/work/out.png")),
                width: 3,
                height: 4,
                format: "png",
            },
            scale: 1.0,
            warnings: Vec::new(),
        };

        assert_eq!(
            success.json(),
            json!({
                "schema_version": SCHEMA_VERSION, "ok": true, "output": "out.png",
                "output_abs": "/work/out.png", "width": 3, "height": 4, "format": "png",
                "scale": 1.0, "warnings": []
            })
        );
        assert_eq!(success.human().as_deref(), Some("out.png"));
    }

    #[test]
    fn stdout_render_prints_nothing_in_human_mode() {
        let success = Success::Rendered {
            result: io::ExportResult {
                output: PathBuf::from("-"),
                output_abs: None,
                width: 1,
                height: 1,
                format: "png",
            },
            scale: 1.0,
            warnings: Vec::new(),
        };

        assert_eq!(success.human(), None);
    }

    #[test]
    fn query_successes_wrap_their_payload_under_one_key() {
        let payload = json!({ "a": 1 });

        assert_eq!(
            Success::Spec {
                spec: payload.clone(),
                warnings: Vec::new()
            }
            .json(),
            json!({
                "schema_version": SCHEMA_VERSION, "ok": true, "spec": payload, "warnings": []
            })
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
        let text = Success::Spec {
            spec: json!({ "a": 1 }),
            warnings: Vec::new(),
        }
        .human()
        .unwrap();

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
            cli.compose_params(None),
        )
        .unwrap();

        let value = spec_json(&spec).unwrap();

        assert_eq!(value["schema_version"], SCHEMA_VERSION);
        assert_eq!(value["width"], 2);
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        std::iter::once("stitchr")
            .chain(values.iter().copied())
            .map(OsString::from)
            .collect()
    }

    fn fixture() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        export_image(
            &DynamicImage::new_rgb8(4, 3),
            directory.path().join("a.png"),
            ExportFormat::Png,
        )
        .unwrap();
        directory
    }

    fn execute_in(directory: &tempfile::TempDir, values: &[&str]) -> Result<Success, CliError> {
        let path = |value: &str| {
            if value.ends_with(".png") || value.ends_with(".jpg") {
                directory.path().join(value).to_string_lossy().into_owned()
            } else {
                value.to_owned()
            }
        };
        let values = values.iter().map(|value| path(value)).collect::<Vec<_>>();
        let values = values.iter().map(String::as_str).collect::<Vec<_>>();
        execute(parse_cli(args(&values)).unwrap())
    }

    fn warning_codes(success: &Success) -> Vec<&'static str> {
        success
            .warnings()
            .iter()
            .map(|warning| warning.code)
            .collect()
    }

    #[test]
    fn parse_cli_records_only_options_given_on_the_command_line() {
        let cli = parse_cli(args(&["a.png", "--spacing", "0", "--layout", "grid"])).unwrap();

        assert_eq!(cli.explicit, ["images", "layout", "spacing"]);
    }

    #[test]
    fn parse_cli_records_nothing_beyond_inputs_for_defaults() {
        let cli = parse_cli(args(&["a.png"])).unwrap();

        assert_eq!(cli.explicit, ["images"]);
    }

    #[test]
    fn print_spec_carries_ignored_option_warnings() {
        let directory = fixture();
        let success = execute_in(
            &directory,
            &[
                "a.png",
                "--layout",
                "split",
                "--spacing",
                "4",
                "--print-spec",
            ],
        )
        .unwrap();

        assert_eq!(warning_codes(&success), ["option_ignored"]);
        assert_eq!(
            success.json()["warnings"],
            json!([{
                "code": "option_ignored",
                "option": "--spacing",
                "reason": "--spacing is not used by the split layout"
            }])
        );
    }

    #[test]
    fn render_carries_label_warnings() {
        let directory = fixture();
        let success = execute_in(
            &directory,
            &[
                "a.png",
                "--label",
                "Far too long for four pixels",
                "-o",
                "out.png",
            ],
        )
        .unwrap();

        assert_eq!(warning_codes(&success), ["label_overflow"]);
        assert!(directory.path().join("out.png").exists());
    }

    #[test]
    fn missing_glyph_warning_comes_after_overflow_for_the_same_label() {
        let directory = fixture();
        let success =
            execute_in(&directory, &["a.png", "--label", "日本", "--print-spec"]).unwrap();

        assert_eq!(
            warning_codes(&success),
            ["label_overflow", "missing_glyphs"]
        );
    }

    #[test]
    fn clean_invocation_has_an_empty_warning_list() {
        let directory = fixture();
        let success = execute_in(&directory, &["a.png", "--print-spec"]).unwrap();

        assert_eq!(success.json()["warnings"], json!([]));
    }

    #[test]
    fn strict_turns_warnings_into_an_argument_error_without_writing() {
        let directory = fixture();
        let error = execute_in(
            &directory,
            &[
                "a.png",
                "--strict",
                "--layout",
                "split",
                "--columns",
                "3",
                "-o",
                "out.png",
            ],
        )
        .unwrap_err();

        assert_eq!(error.code(), "invalid_arguments");
        assert_eq!(error.exit_code(), 2);
        assert!(!directory.path().join("out.png").exists());
    }

    #[test]
    fn strict_error_lists_every_warning_reason() {
        let directory = fixture();
        let error = execute_in(
            &directory,
            &[
                "a.png",
                "--strict",
                "--layout",
                "split",
                "--columns",
                "3",
                "--spacing",
                "1",
                "--print-spec",
            ],
        )
        .unwrap_err();

        assert_eq!(
            error.message(),
            "--strict treats warnings as errors: --columns is not used by the split layout; --spacing is not used by the split layout"
        );
    }

    #[test]
    fn strict_without_warnings_renders_normally() {
        let directory = fixture();
        let success = execute_in(&directory, &["a.png", "--strict", "-o", "out.png"]).unwrap();

        assert_eq!(warning_codes(&success), Vec::<&str>::new());
        assert!(directory.path().join("out.png").exists());
    }

    #[test]
    fn strict_also_applies_to_spec_inspection() {
        let directory = fixture();

        assert!(execute_in(
            &directory,
            &["a.png", "--strict", "--divider", "2", "--print-spec"]
        )
        .is_err());
    }

    #[test]
    fn schema_query_ignores_warnings_and_strict() {
        let success =
            execute(parse_cli(args(&["--help-json", "--strict", "--divider", "3"])).unwrap())
                .unwrap();

        assert!(matches!(success, Success::Schema(_)));
        assert_eq!(success.warnings().len(), 0);
    }

    #[test]
    fn fit_target_accepts_min_and_positive_pixels() {
        assert_eq!(fit_target("min"), Ok(FitTarget::Min));
        assert_eq!(fit_target("1"), Ok(FitTarget::Pixels(1)));
        assert_eq!(fit_target("4096"), Ok(FitTarget::Pixels(4096)));
    }

    #[test]
    fn fit_target_rejects_zero_negatives_and_words() {
        for value in ["0", "-1", "MIN", "max", "", "1.5"] {
            assert_eq!(
                fit_target(value),
                Err("must be a positive integer or min".to_owned()),
                "{value:?}"
            );
        }
    }

    fn metadata(sizes: &[(u32, u32)]) -> Vec<ImageMetadata> {
        sizes
            .iter()
            .map(|&(width, height)| ImageMetadata {
                name: "image.png".into(),
                width,
                height,
            })
            .collect()
    }

    #[test]
    fn fitted_sizes_use_the_split_target_only_with_fit() {
        let images = metadata(&[(40, 20), (30, 30)]);

        assert_eq!(
            fitted_sizes(
                &parse_cli(args(&["a", "b", "--layout", "split", "--fit", "cover"])).unwrap(),
                &images
            ),
            [(30, 20), (30, 20)]
        );
        assert_eq!(
            fitted_sizes(
                &parse_cli(args(&["a", "b", "--layout", "split"])).unwrap(),
                &images
            ),
            [(40, 20), (30, 30)]
        );
    }

    #[test]
    fn fitted_sizes_ignore_fit_height_for_split() {
        let images = metadata(&[(40, 20), (40, 20)]);

        assert_eq!(
            fitted_sizes(
                &parse_cli(args(&["a", "b", "--layout", "split", "--fit-height", "10"])).unwrap(),
                &images
            ),
            [(40, 20), (40, 20)]
        );
    }

    #[test]
    fn fitted_sizes_apply_fit_height_to_stacks() {
        let images = metadata(&[(40, 20), (10, 10)]);

        assert_eq!(
            fitted_sizes(
                &parse_cli(args(&["a", "b", "--fit-height", "min"])).unwrap(),
                &images
            ),
            [(20, 10), (10, 10)]
        );
    }
}
