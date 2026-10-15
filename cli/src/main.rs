mod composer;

use clap::{Arg, ArgAction, CommandFactory, Parser, ValueEnum, ValueHint};
use composer::{CliComposeParams, CliComposer};
use serde_json::{json, Value};
use std::{path::PathBuf, process::ExitCode};
use stitchr_core::{
    export_image, load_image_with_metadata, render_spec, CompositionSpec, DynamicImage,
    ExportFormat, LoadedImage, RgbColor,
};

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

#[derive(Debug, Parser)]
#[command(
    name = "stitchr",
    version,
    about = "Combine PNG and JPEG images into one image",
    long_about = "Arrange PNG/JPEG images in a grid, horizontal or vertical stack, at explicit freeform positions, or as diagonal split slices of same-size images. Output format is selected by the output file extension.",
    after_help = "Examples:\n  stitchr img1.png img2.png --layout grid --spacing 10\n  stitchr img1.png img2.jpg --layout horizontal --alignment center -o combined.jpg\n  stitchr img1.png img2.png --layout freeform --position 0 0 --position 300 80 -o combined.png\n  stitchr light.png dark.png --layout split --divider 4 --background white -o split.png\n  stitchr img1.png img2.png --background rebeccapurple --print-spec"
)]
struct Cli {
    #[arg(required_unless_present = "help_json", num_args = 1.., value_name = "IMAGE", help = "Input PNG/JPEG paths (one or more; required unless --help-json/--schema)")]
    images: Vec<PathBuf>,

    #[arg(
        long,
        value_enum,
        default_value = "grid",
        help = "Layout: grid, horizontal stack, vertical stack, freeform, or diagonal split of same-size images"
    )]
    layout: CliLayout,

    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..), value_name = "COUNT", help = "Grid columns (positive integer; only for grid)")]
    columns: u32,

    #[arg(
        long,
        default_value_t = 0,
        value_name = "PIXELS",
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

    #[arg(long, action = ArgAction::Append, num_args = 2, value_names = ["X", "Y"], help = "Freeform top-left coordinates (nonnegative integers; repeat once per image)")]
    position: Vec<u32>,

    #[arg(
        long,
        default_value_t = 0,
        value_name = "PIXELS",
        help = "Width of the background-colored line between split images (nonnegative integer; only for split)"
    )]
    divider: u32,

    #[arg(
        long,
        short,
        default_value = "stitched.png",
        value_name = "FILE",
        help = "Output file (.png, .jpg, or .jpeg; default: stitched.png)"
    )]
    output: PathBuf,

    #[arg(long = "help-json", visible_alias = "schema", action = ArgAction::SetTrue, help = "Print the full CLI specification as JSON (boolean; default: false)")]
    help_json: bool,

    #[arg(long = "print-spec", action = ArgAction::SetTrue, help = "Print the composition spec as JSON after loading inputs (boolean; default: false)")]
    print_spec: bool,
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
    let color = arg.get_id().as_str() == "background_color";
    let value_type = if boolean {
        "boolean"
    } else if path || color || !choices.is_empty() {
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
        "name": command.get_name(),
        "version": command.get_version(),
        "description": command.get_long_about().map(ToString::to_string),
        "usage": usage,
        "examples": examples,
        "arguments": arguments
    })
}

fn output_format(output: &std::path::Path) -> Result<ExportFormat, String> {
    match output.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("png") => Ok(ExportFormat::Png),
        Some(extension)
            if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") =>
        {
            Ok(ExportFormat::Jpeg)
        }
        _ => Err("output filename must end in .png, .jpg, or .jpeg".to_owned()),
    }
}

fn load_images(paths: &[PathBuf]) -> Result<Vec<LoadedImage>, String> {
    paths
        .iter()
        .map(|path| {
            load_image_with_metadata(path)
                .map_err(|error| format!("cannot load {}: {error}", path.display()))
        })
        .collect()
}

fn compose_loaded_images(
    loaded: &[LoadedImage],
    params: CliComposeParams<'_>,
) -> Result<CompositionSpec, String> {
    let metadata = loaded
        .iter()
        .map(|image| image.metadata.clone())
        .collect::<Vec<_>>();
    CliComposer::compose(&metadata, params)
}

fn run(output: PathBuf, composed: DynamicImage) -> Result<PathBuf, String> {
    let format = output_format(&output)?;
    export_image(&composed, &output, format)
        .map_err(|error| format!("cannot write {}: {error}", output.display()))?;
    Ok(output)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if cli.help_json {
        println!("{}", schema());
        return ExitCode::SUCCESS;
    }

    let loaded = match load_images(&cli.images) {
        Ok(images) => images,
        Err(error) => {
            eprintln!("stitchr: {error}");
            return ExitCode::FAILURE;
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
        },
    ) {
        Ok(spec) => spec,
        Err(error) => {
            eprintln!("stitchr: {error}");
            return ExitCode::FAILURE;
        }
    };
    if cli.print_spec {
        return match serde_json::to_string_pretty(&spec) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("stitchr: cannot print spec: {error}");
                ExitCode::FAILURE
            }
        };
    }

    let images = loaded
        .into_iter()
        .map(|loaded| loaded.image)
        .collect::<Vec<_>>();
    let composed = match render_spec(&spec, &images) {
        Ok(image) => image,
        Err(error) => {
            eprintln!("stitchr: {error}");
            return ExitCode::FAILURE;
        }
    };
    match run(cli.output, composed) {
        Ok(output) => {
            println!("{}", output.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("stitchr: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stitchr_core::DynamicImage;

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
            },
        )
        .unwrap();
        let pixels = loaded
            .iter()
            .map(|image| image.image.clone())
            .collect::<Vec<_>>();
        let composed = render_spec(&spec, &pixels).unwrap();
        assert_eq!(run(freeform.output.clone(), composed).unwrap(), png);
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
            },
        )
        .unwrap();
        let pixels = loaded
            .iter()
            .map(|image| image.image.clone())
            .collect::<Vec<_>>();
        let composed = render_spec(&spec, &pixels).unwrap();
        assert_eq!(run(vertical.output.clone(), composed).unwrap(), jpeg);
        let result = stitchr_core::load_image(&jpeg).unwrap();
        assert_eq!((result.width(), result.height()), (6, 8));
        assert!(std::fs::read(&jpeg)
            .unwrap()
            .starts_with(&[0xff, 0xd8, 0xff]));
    }
}
