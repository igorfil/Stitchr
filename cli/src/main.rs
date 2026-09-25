use clap::{Arg, ArgAction, CommandFactory, Parser, ValueEnum, ValueHint};
use serde_json::{json, Value};
use std::{path::PathBuf, process::ExitCode};
use stitchr_core::{
    export_image, render_paths, Alignment, CompositionSpec, ExportFormat, RgbColor, SpecLayout,
    SpecPosition, StackDirection,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliLayout {
    Grid,
    Horizontal,
    Vertical,
    Freeform,
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
    long_about = "Arrange PNG/JPEG images in a grid, horizontal or vertical stack, or at explicit freeform positions. Output format is selected by the output file extension.",
    after_help = "Examples:\n  stitchr img1.png img2.png --layout grid --spacing 10\n  stitchr img1.png img2.jpg --layout horizontal --alignment center -o combined.jpg\n  stitchr img1.png img2.png --layout freeform --position 0 0 --position 300 80 -o combined.png\n  stitchr img1.png img2.png --background rebeccapurple --print-spec"
)]
struct Cli {
    #[arg(required_unless_present = "help_json", num_args = 1.., value_name = "IMAGE", help = "Input PNG/JPEG paths (one or more; required unless --help-json/--schema)")]
    images: Vec<PathBuf>,

    #[arg(
        long,
        value_enum,
        default_value = "grid",
        help = "Layout: grid, horizontal stack, vertical stack, or freeform"
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
        short,
        default_value = "stitched.png",
        value_name = "FILE",
        help = "Output file (.png, .jpg, or .jpeg; default: stitched.png)"
    )]
    output: PathBuf,

    #[arg(long = "help-json", visible_alias = "schema", action = ArgAction::SetTrue, help = "Print the full CLI specification as JSON (boolean; default: false)")]
    help_json: bool,

    #[arg(long = "print-spec", action = ArgAction::SetTrue, help = "Print the composition spec as JSON without loading images (boolean; default: false)")]
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

fn build_spec(cli: &Cli) -> Result<CompositionSpec, String> {
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

fn run(cli: Cli, spec: CompositionSpec) -> Result<PathBuf, String> {
    let format = match cli
        .output
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) if extension.eq_ignore_ascii_case("png") => ExportFormat::Png,
        Some(extension)
            if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") =>
        {
            ExportFormat::Jpeg
        }
        _ => return Err("output filename must end in .png, .jpg, or .jpeg".to_owned()),
    };
    let composed = render_paths(&spec).map_err(|error| error.to_string())?;
    export_image(&composed, &cli.output, format)
        .map_err(|error| format!("cannot write {}: {error}", cli.output.display()))?;
    Ok(cli.output)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if cli.help_json {
        println!("{}", schema());
        return ExitCode::SUCCESS;
    }

    let spec = match build_spec(&cli) {
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

    match run(cli, spec) {
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
        let spec = build_spec(&cli).unwrap();

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
        let spec = build_spec(&cli).unwrap();

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

        let named_color = build_spec(&named).unwrap().background_color;
        let rgb_color = build_spec(&rgb).unwrap().background_color;
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
    fn help_lists_options_defaults_and_examples() {
        let help = Cli::try_parse_from(["stitchr", "--help"])
            .unwrap_err()
            .to_string();

        assert!(help.contains("[possible values: grid, horizontal, vertical, freeform]"));
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
            json!(["grid", "horizontal", "vertical", "freeform"])
        );
        assert_eq!(by_name("layout")["default"], "grid");
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
        let spec = build_spec(&freeform).unwrap();
        assert_eq!(run(freeform, spec).unwrap(), png);
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
        let spec = build_spec(&vertical).unwrap();
        assert_eq!(run(vertical, spec).unwrap(), jpeg);
        let result = stitchr_core::load_image(&jpeg).unwrap();
        assert_eq!((result.width(), result.height()), (6, 8));
        assert!(std::fs::read(&jpeg)
            .unwrap()
            .starts_with(&[0xff, 0xd8, 0xff]));
    }
}
