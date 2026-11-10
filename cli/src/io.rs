use crate::{error::CliError, CliFormat};
use serde_json::json;
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
};
use stitchr_core::{
    load_image_from_bytes, load_image_with_metadata, write_image, DynamicImage, ExportFormat,
    ImageMetadata, LoadedImage,
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

/// Whether two existing paths name the same file, including through symlinks and hard links.
fn same_file(first: &Path, second: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(first), Ok(second)) = (std::fs::metadata(first), std::fs::metadata(second)) {
            return (first.dev(), first.ino()) == (second.dev(), second.ino());
        }
    }
    match (first.canonicalize(), second.canonicalize()) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

/// Refuses to overwrite an input unless `force`, and any existing file with `no_clobber`.
pub(crate) fn validate_destination(
    output: &Path,
    inputs: &[PathBuf],
    force: bool,
    no_clobber: bool,
) -> Result<(), CliError> {
    if is_stdio(output) {
        return Ok(());
    }
    let conflict = inputs
        .iter()
        .enumerate()
        .find(|(_, input)| !is_stdio(input) && same_file(input, output));
    if let (Some((index, input)), false) = (conflict, force) {
        return Err(CliError::new(
            "output_conflicts_with_input",
            format!(
                "output {} is also input {index} ({}); pass --force to overwrite it",
                output.display(),
                input.display()
            ),
        )
        .with_details(json!({
            "path": output.to_string_lossy(),
            "index": index,
            "input": input.to_string_lossy()
        })));
    }
    if no_clobber && output.exists() {
        return Err(output_exists(output));
    }
    Ok(())
}

fn output_exists(output: &Path) -> CliError {
    CliError::new(
        "output_exists",
        format!(
            "{} already exists; remove --no-clobber to overwrite it",
            output.display()
        ),
    )
    .with_details(json!({ "path": output.to_string_lossy() }))
}

fn output_error(output: &Path, error: impl std::fmt::Display) -> CliError {
    CliError::new(
        "output_error",
        format!("cannot write {}: {error}", output.display()),
    )
    .with_details(json!({ "path": output.to_string_lossy() }))
}

pub(crate) struct Destination {
    pub path: PathBuf,
    pub format: Option<CliFormat>,
    pub jpeg_quality: u8,
    pub no_clobber: bool,
    pub mkdir: bool,
}

fn open_file(destination: &Destination) -> Result<File, CliError> {
    let path = destination.path.as_path();
    if destination.mkdir {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| output_error(path, error))?;
        }
    }
    let mut options = OpenOptions::new();
    options.write(true);
    if destination.no_clobber {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    options.open(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::AlreadyExists => output_exists(path),
        _ => output_error(path, error),
    })
}

fn write_file(
    destination: &Destination,
    composed: &DynamicImage,
    format: ExportFormat,
) -> Result<(), CliError> {
    let path = destination.path.as_path();
    let mut file = BufWriter::new(open_file(destination)?);
    write_image(composed, &mut file, format, destination.jpeg_quality)
        .map_err(|error| output_error(path, error))?;
    file.flush().map_err(|error| output_error(path, error))
}

fn write_stdout(
    composed: &DynamicImage,
    format: ExportFormat,
    jpeg_quality: u8,
) -> Result<(), CliError> {
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
    pub output_abs: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub format: &'static str,
}

pub(crate) fn export(
    destination: Destination,
    composed: &DynamicImage,
) -> Result<ExportResult, CliError> {
    let format = output_format(&destination.path, destination.format)?;
    let output_abs = if is_stdio(&destination.path) {
        write_stdout(composed, format, destination.jpeg_quality)?;
        None
    } else {
        write_file(&destination, composed, format)?;
        std::path::absolute(&destination.path).ok()
    };
    Ok(ExportResult {
        output: destination.path,
        output_abs,
        width: composed.width(),
        height: composed.height(),
        format: match format {
            ExportFormat::Png => "png",
            ExportFormat::Jpeg => "jpeg",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stitchr_core::{export_image, load_image};

    fn image(directory: &Path, name: &str) -> PathBuf {
        let path = directory.join(name);
        export_image(&DynamicImage::new_rgb8(2, 2), &path, ExportFormat::Png).unwrap();
        path
    }

    fn destination(path: PathBuf) -> Destination {
        Destination {
            path,
            format: None,
            jpeg_quality: 75,
            no_clobber: false,
            mkdir: false,
        }
    }

    fn code(result: Result<(), CliError>) -> &'static str {
        result.unwrap_err().code()
    }

    #[test]
    fn new_output_never_conflicts() {
        let directory = tempfile::tempdir().unwrap();
        let input = image(directory.path(), "a.png");

        assert!(
            validate_destination(&directory.path().join("out.png"), &[input], false, false).is_ok()
        );
    }

    #[test]
    fn output_equal_to_an_input_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let first = image(directory.path(), "a.png");
        let second = image(directory.path(), "b.png");

        let error =
            validate_destination(&second, &[first, second.clone()], false, false).unwrap_err();

        assert_eq!(error.code(), "output_conflicts_with_input");
        assert_eq!(error.details().unwrap()["index"], 1);
    }

    #[test]
    fn output_spelled_differently_but_naming_an_input_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let input = image(directory.path(), "a.png");
        std::fs::create_dir(directory.path().join("sub")).unwrap();
        let roundabout = directory.path().join("sub").join("..").join("a.png");

        assert_eq!(
            code(validate_destination(&roundabout, &[input], false, false)),
            "output_conflicts_with_input"
        );
    }

    #[cfg(unix)]
    #[test]
    fn output_symlinked_to_an_input_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let input = image(directory.path(), "a.png");
        let link = directory.path().join("link.png");
        std::os::unix::fs::symlink(&input, &link).unwrap();

        assert_eq!(
            code(validate_destination(&link, &[input], false, false)),
            "output_conflicts_with_input"
        );
    }

    #[cfg(unix)]
    #[test]
    fn output_hard_linked_to_an_input_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let input = image(directory.path(), "a.png");
        let link = directory.path().join("hard.png");
        std::fs::hard_link(&input, &link).unwrap();

        assert_eq!(
            code(validate_destination(&link, &[input], false, false)),
            "output_conflicts_with_input"
        );
    }

    #[test]
    fn force_allows_overwriting_an_input() {
        let directory = tempfile::tempdir().unwrap();
        let input = image(directory.path(), "a.png");

        assert!(validate_destination(&input, std::slice::from_ref(&input), true, false).is_ok());
    }

    #[test]
    fn stdin_input_and_stdout_output_never_conflict() {
        let directory = tempfile::tempdir().unwrap();
        let input = image(directory.path(), "a.png");

        assert!(validate_destination(Path::new("-"), &[input], false, true).is_ok());
        assert!(validate_destination(
            &directory.path().join("out.png"),
            &[PathBuf::from("-")],
            false,
            false
        )
        .is_ok());
    }

    #[test]
    fn existing_output_is_allowed_by_default() {
        let directory = tempfile::tempdir().unwrap();
        let input = image(directory.path(), "a.png");
        let existing = image(directory.path(), "existing.png");

        assert!(validate_destination(&existing, &[input], false, false).is_ok());
    }

    #[test]
    fn no_clobber_refuses_an_existing_output() {
        let directory = tempfile::tempdir().unwrap();
        let input = image(directory.path(), "a.png");
        let existing = image(directory.path(), "existing.png");

        assert_eq!(
            code(validate_destination(&existing, &[input], false, true)),
            "output_exists"
        );
    }

    #[test]
    fn no_clobber_with_force_still_refuses_an_input_as_output() {
        let directory = tempfile::tempdir().unwrap();
        let input = image(directory.path(), "a.png");

        assert_eq!(
            code(validate_destination(
                &input,
                std::slice::from_ref(&input),
                true,
                true
            )),
            "output_exists"
        );
    }

    #[test]
    fn export_reports_an_absolute_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("out.png");

        let result = export(destination(path.clone()), &DynamicImage::new_rgb8(3, 1)).unwrap();

        assert_eq!(result.output, path);
        assert!(result.output_abs.unwrap().is_absolute());
        assert_eq!(load_image(&path).unwrap().width(), 3);
    }

    #[test]
    fn export_overwrites_an_existing_file_by_default() {
        let directory = tempfile::tempdir().unwrap();
        let path = image(directory.path(), "out.png");

        export(destination(path.clone()), &DynamicImage::new_rgb8(5, 1)).unwrap();

        assert_eq!(load_image(&path).unwrap().width(), 5);
    }

    #[test]
    fn export_with_no_clobber_keeps_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = image(directory.path(), "out.png");

        let error = export(
            Destination {
                no_clobber: true,
                ..destination(path.clone())
            },
            &DynamicImage::new_rgb8(5, 1),
        )
        .unwrap_err();

        assert_eq!(error.code(), "output_exists");
        assert_eq!(load_image(&path).unwrap().width(), 2);
    }

    #[test]
    fn export_without_mkdir_fails_for_a_missing_directory() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing").join("out.png");

        let error = export(destination(path), &DynamicImage::new_rgb8(1, 1)).unwrap_err();

        assert_eq!(error.code(), "output_error");
    }

    #[test]
    fn export_with_mkdir_creates_nested_directories() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("a").join("b").join("out.png");

        export(
            Destination {
                mkdir: true,
                ..destination(path.clone())
            },
            &DynamicImage::new_rgb8(1, 1),
        )
        .unwrap();

        assert!(path.exists());
    }

    #[test]
    fn mkdir_with_a_bare_file_name_needs_no_directory() {
        let directory = tempfile::tempdir().unwrap();
        let open = open_file(&Destination {
            mkdir: true,
            ..destination(directory.path().join("out.png"))
        });

        assert!(open.is_ok());
    }

    #[test]
    fn mkdir_reports_a_parent_that_is_a_file() {
        let directory = tempfile::tempdir().unwrap();
        let blocker = image(directory.path(), "file.png");

        let error = export(
            Destination {
                mkdir: true,
                ..destination(blocker.join("out.png"))
            },
            &DynamicImage::new_rgb8(1, 1),
        )
        .unwrap_err();

        assert_eq!(error.code(), "output_error");
    }
}
