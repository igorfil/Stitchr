use clap::{ArgAction, Parser, ValueEnum};
use std::{path::PathBuf, process::ExitCode};
use stitchr_core::{
    compose, export_image, load_image, Alignment, ComposeOptions, ExportFormat, Layout,
    StackDirection,
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
    after_help = "Examples:\n  stitchr img1.png img2.png --layout grid --max-size 2000 --spacing 10\n  stitchr img1.png img2.jpg --layout horizontal --alignment center -o combined.jpg\n  stitchr img1.png img2.png --layout freeform --position 0 0 --position 300 80 -o combined.png"
)]
struct Cli {
    #[arg(required = true, num_args = 1.., value_name = "IMAGE", help = "Input PNG/JPEG file paths (one or more)")]
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
        help = "Space between grid cells or stacked images (nonnegative integer)"
    )]
    spacing: u32,

    #[arg(
        long,
        value_enum,
        default_value = "start",
        help = "Align images within cells or across a stack: start, center, end"
    )]
    alignment: CliAlignment,

    #[arg(long, value_parser = clap::value_parser!(u32).range(1..), value_name = "PIXELS", help = "Maximum output width or height (positive integer; default: no limit)")]
    max_size: Option<u32>,

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
}

fn run(cli: Cli) -> Result<PathBuf, String> {
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
    let images = cli
        .images
        .iter()
        .map(|path| {
            load_image(path).map_err(|error| format!("cannot load {}: {error}", path.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let layout = match cli.layout {
        CliLayout::Grid => Layout::Grid {
            columns: cli.columns as usize,
        },
        CliLayout::Horizontal => Layout::Stack {
            direction: StackDirection::Horizontal,
        },
        CliLayout::Vertical => Layout::Stack {
            direction: StackDirection::Vertical,
        },
        CliLayout::Freeform => Layout::Freeform {
            positions: cli
                .position
                .chunks_exact(2)
                .map(|pair| (pair[0], pair[1]))
                .collect(),
        },
    };
    let alignment = match cli.alignment {
        CliAlignment::Start => Alignment::Start,
        CliAlignment::Center => Alignment::Center,
        CliAlignment::End => Alignment::End,
    };
    let options = ComposeOptions {
        layout,
        spacing: cli.spacing,
        alignment,
        max_size: cli.max_size,
    };
    let composed = compose(&images, &options).map_err(|error| error.to_string())?;
    export_image(&composed, &cli.output, format)
        .map_err(|error| format!("cannot write {}: {error}", cli.output.display()))?;
    Ok(cli.output)
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
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
            "--max-size",
            "2000",
            "--spacing",
            "10",
        ])
        .unwrap();

        assert_eq!(
            cli.images,
            [PathBuf::from("one.png"), PathBuf::from("two.jpg")]
        );
        assert_eq!(cli.layout, CliLayout::Grid);
        assert_eq!(cli.max_size, Some(2000));
        assert_eq!(cli.spacing, 10);
        assert_eq!(cli.columns, 2);
        assert_eq!(cli.output, PathBuf::from("stitched.png"));
        assert!(Cli::try_parse_from(["stitchr"]).is_err());
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
        assert_eq!(run(freeform).unwrap(), png);
        let result = load_image(&png).unwrap();
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
        assert_eq!(run(vertical).unwrap(), jpeg);
        let result = load_image(&jpeg).unwrap();
        assert_eq!((result.width(), result.height()), (2, 4));
        assert!(std::fs::read(&jpeg)
            .unwrap()
            .starts_with(&[0xff, 0xd8, 0xff]));
    }
}
