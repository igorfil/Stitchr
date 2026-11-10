use crate::{error::CliError, CliFormat};
use serde_json::json;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
use stitchr_core::{
    export_image_with_quality, load_image_from_bytes, load_image_with_metadata, write_image,
    DynamicImage, ExportFormat, ImageMetadata, LoadedImage,
};

pub(crate) const STDIO_PATH: &str = "-";
const SUPPORTED_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

pub(crate) fn is_stdio(path: &Path) -> bool {
    path.as_os_str() == STDIO_PATH
}

pub(crate) fn output_format(
    output: &Path,
    format: Option<CliFormat>,
) -> Result<ExportFormat, CliError> {
    match format {
        Some(CliFormat::Png) => return Ok(ExportFormat::Png),
        Some(CliFormat::Jpeg) => return Ok(ExportFormat::Jpeg),
        None if is_stdio(output) => {
            return Err(CliError::new(
                "unsupported_output_format",
                "-o - requires --format png or --format jpeg",
            )
            .with_details(json!({ "path": STDIO_PATH, "supported_formats": ["png", "jpeg"] })))
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
        )
        .with_details(json!({
            "path": output.to_string_lossy(),
            "supported_extensions": SUPPORTED_EXTENSIONS
        }))),
    }
}

fn input_error(index: usize, path: &Path, message: String) -> CliError {
    CliError::new("input_error", message)
        .with_details(json!({ "index": index, "path": path.to_string_lossy() }))
}

fn load_stdin(index: usize) -> Result<LoadedImage, CliError> {
    let stdin = Path::new(STDIO_PATH);
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .read_to_end(&mut bytes)
        .map_err(|error| input_error(index, stdin, format!("cannot read stdin: {error}")))?;
    let image = load_image_from_bytes(&bytes)
        .map_err(|error| input_error(index, stdin, format!("cannot load stdin: {error}")))?;
    let metadata = ImageMetadata {
        name: STDIO_PATH.into(),
        width: image.width(),
        height: image.height(),
    };
    Ok(LoadedImage { image, metadata })
}

pub(crate) fn load_images(paths: &[PathBuf]) -> Result<Vec<LoadedImage>, CliError> {
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            if is_stdio(path) {
                return load_stdin(index);
            }
            load_image_with_metadata(path).map_err(|error| {
                input_error(
                    index,
                    path,
                    format!("cannot load {}: {error}", path.display()),
                )
            })
        })
        .collect()
}

pub(crate) fn validate_stdin(images: &[PathBuf]) -> Result<(), CliError> {
    if images.iter().filter(|path| is_stdio(path)).count() > 1 {
        return Err(CliError::new(
            "invalid_arguments",
            "- (stdin) can be used as an input only once",
        )
        .with_details(json!({ "kind": "stdin_reused", "option": STDIO_PATH })));
    }
    Ok(())
}

pub(crate) fn validate_stdout(
    output: &Path,
    format: Option<CliFormat>,
    json: bool,
) -> Result<(), CliError> {
    if !is_stdio(output) {
        return Ok(());
    }
    if json {
        return Err(CliError::new(
            "invalid_arguments",
            "-o - cannot be combined with --json because stdout carries the image",
        )
        .with_details(json!({ "kind": "argument_conflict", "option": "--output", "conflicts_with": "--json" })));
    }
    if format.is_none() {
        return Err(CliError::new(
            "invalid_arguments",
            "-o - requires --format png or --format jpeg",
        )
        .with_details(json!({ "kind": "missing_required_argument", "option": "--format", "required_by": "--output -" })));
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
                .with_details(json!({ "path": output.to_string_lossy() }))
            },
        );
    }
    // Encode fully before writing so an encoder failure never leaves a truncated image on stdout.
    let mut bytes = Vec::new();
    write_image(composed, &mut bytes, format, jpeg_quality).map_err(|error| {
        CliError::new("output_error", format!("cannot encode image: {error}"))
            .with_details(json!({ "path": STDIO_PATH }))
    })?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&bytes)
        .and_then(|()| stdout.flush())
        .map_err(|error| {
            CliError::new("output_error", format!("cannot write stdout: {error}"))
                .with_details(json!({ "path": STDIO_PATH }))
        })
}

#[derive(Debug)]
pub(crate) struct ExportResult {
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    pub format: &'static str,
}

pub(crate) fn export(
    output: PathBuf,
    composed: &DynamicImage,
    format: Option<CliFormat>,
    jpeg_quality: u8,
) -> Result<ExportResult, CliError> {
    let format = output_format(&output, format)?;
    write_output(&output, composed, format, jpeg_quality)?;
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
