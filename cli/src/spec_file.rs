use crate::{error::CliError, io, SCHEMA_VERSION};
use serde_json::{json, Value};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use stitchr_core::{fit::check_source_size, validate_spec, CompositionSpec, ImageMetadata};

fn invalid_spec(message: impl Into<String>, details: Value) -> CliError {
    CliError::new("invalid_spec", message).with_details(details)
}

fn read_source(source: &Path) -> Result<String, CliError> {
    let path = source.to_string_lossy();
    let mut text = String::new();
    let read = if io::is_stdio(source) {
        std::io::stdin()
            .lock()
            .read_to_string(&mut text)
            .map(|_| ())
    } else {
        std::fs::read_to_string(source).map(|contents| text = contents)
    };
    read.map_err(|error| {
        CliError::new("input_error", format!("cannot read spec {path}: {error}"))
            .with_details(json!({ "path": path }))
    })?;
    Ok(text)
}

/// Parses and checks a spec printed by `--print-spec`.
pub(crate) fn parse(text: &str, source: &Path) -> Result<CompositionSpec, CliError> {
    let path = source.to_string_lossy();
    let value: Value = serde_json::from_str(text).map_err(|error| {
        invalid_spec(
            format!("spec {path} is not valid JSON: {error}"),
            json!({ "path": path, "reason": error.to_string() }),
        )
    })?;
    match value.get("schema_version") {
        None => {}
        Some(version) if *version == json!(SCHEMA_VERSION) => {}
        Some(version) => {
            return Err(invalid_spec(
                format!("spec {path} has schema_version {version}, expected {SCHEMA_VERSION}"),
                json!({ "path": path, "schema_version": version, "expected": SCHEMA_VERSION }),
            ))
        }
    }
    let spec: CompositionSpec = serde_json::from_value(value).map_err(|error| {
        invalid_spec(
            format!("spec {path} is not a composition spec: {error}"),
            json!({ "path": path, "reason": error.to_string() }),
        )
    })?;
    if let Some(index) = spec.images.iter().position(|image| image.path.is_none()) {
        return Err(invalid_spec(
            format!("spec {path}: image {index} has no path"),
            json!({ "path": path, "index": index, "reason": "missing path" }),
        ));
    }
    if io::is_stdio(source) {
        if let Some(index) = spec
            .images
            .iter()
            .position(|image| image.path.as_deref() == Some("-"))
        {
            return Err(invalid_spec(
                format!("spec read from stdin cannot also read image {index} from stdin"),
                json!({ "path": path, "index": index, "reason": "stdin used twice" }),
            ));
        }
    }
    validate_spec(&spec)
        .map_err(|error| CliError::from(error).with_input_paths(&input_paths(&spec)))?;
    Ok(spec)
}

pub(crate) fn read(source: &Path) -> Result<CompositionSpec, CliError> {
    parse(&read_source(source)?, source)
}

pub(crate) fn input_paths(spec: &CompositionSpec) -> Vec<PathBuf> {
    spec.images
        .iter()
        .map(|image| PathBuf::from(image.path.clone().unwrap_or_default()))
        .collect()
}

/// Fails when a loaded input would have to be enlarged to the size its placement expects.
pub(crate) fn check_sizes(
    spec: &CompositionSpec,
    metadata: &[ImageMetadata],
) -> Result<(), CliError> {
    spec.images
        .iter()
        .zip(metadata)
        .enumerate()
        .try_for_each(|(index, (placement, image))| {
            check_source_size(index, placement, (image.width, image.height))
        })
        .map_err(|error| CliError::from(error).with_input_paths(&input_paths(spec)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_text(images: Value) -> String {
        json!({
            "schema_version": 1,
            "images": images,
            "layout": { "type": "grid", "columns": 2, "spacing": 0, "alignment": "start" },
            "width": 4,
            "height": 2,
            "background_color": { "red": 0, "green": 0, "blue": 0 }
        })
        .to_string()
    }

    fn image(path: &str) -> Value {
        json!({ "name": "a.png", "path": path, "width": 2, "height": 2, "position": { "x": 0, "y": 0 } })
    }

    fn file() -> &'static Path {
        Path::new("spec.json")
    }

    fn error_code(text: &str, source: &Path) -> &'static str {
        parse(text, source).unwrap_err().code()
    }

    #[test]
    fn printed_spec_parses() {
        let spec = parse(&spec_text(json!([image("a.png"), image("b.png")])), file()).unwrap();

        assert_eq!(spec.images.len(), 2);
        assert_eq!(
            input_paths(&spec),
            [PathBuf::from("a.png"), PathBuf::from("b.png")]
        );
    }

    #[test]
    fn spec_without_schema_version_is_accepted() {
        let mut value: Value = serde_json::from_str(&spec_text(json!([image("a.png")]))).unwrap();
        value.as_object_mut().unwrap().remove("schema_version");

        assert!(parse(&value.to_string(), file()).is_ok());
    }

    #[test]
    fn unknown_schema_version_is_rejected() {
        let text = spec_text(json!([image("a.png")]))
            .replace("\"schema_version\":1", "\"schema_version\":2");
        let error = parse(&text, file()).unwrap_err();

        assert_eq!(error.code(), "invalid_spec");
        assert_eq!(error.details().unwrap()["schema_version"], 2);
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn malformed_json_is_an_invalid_spec() {
        assert_eq!(error_code("{ not json", file()), "invalid_spec");
    }

    #[test]
    fn wrong_shape_is_an_invalid_spec() {
        assert_eq!(error_code("{\"images\": 3}", file()), "invalid_spec");
        assert_eq!(error_code("[]", file()), "invalid_spec");
    }

    #[test]
    fn image_without_a_path_is_an_invalid_spec() {
        let mut without_path = image("a.png");
        without_path.as_object_mut().unwrap().remove("path");
        let error = parse(&spec_text(json!([image("a.png"), without_path])), file()).unwrap_err();

        assert_eq!(error.code(), "invalid_spec");
        assert_eq!(error.details().unwrap()["index"], 1);
    }

    #[test]
    fn stdin_image_is_allowed_in_a_spec_file() {
        assert!(parse(&spec_text(json!([image("-")])), file()).is_ok());
    }

    #[test]
    fn stdin_spec_cannot_also_read_an_image_from_stdin() {
        let error = parse(&spec_text(json!([image("-")])), Path::new("-")).unwrap_err();

        assert_eq!(error.code(), "invalid_spec");
        assert_eq!(error.details().unwrap()["reason"], "stdin used twice");
    }

    #[test]
    fn inconsistent_spec_reports_the_engine_error() {
        assert_eq!(error_code(&spec_text(json!([])), file()), "no_images");
    }

    #[test]
    fn split_spec_with_mismatched_images_names_the_input() {
        let text = json!({
            "images": [image("a.png"), { "name": "b", "path": "b.png", "width": 3, "height": 2, "position": { "x": 0, "y": 0 } }],
            "layout": { "type": "split", "divider": 0 },
            "width": 2,
            "height": 2,
            "background_color": { "red": 0, "green": 0, "blue": 0 }
        })
        .to_string();
        let error = parse(&text, file()).unwrap_err();

        assert_eq!(error.code(), "split_size_mismatch");
        assert_eq!(error.details().unwrap()["path"], "b.png");
    }

    #[test]
    fn missing_spec_file_is_an_input_error() {
        let error = read(Path::new("/definitely/missing/spec.json")).unwrap_err();

        assert_eq!(error.code(), "input_error");
        assert_eq!(
            error.details().unwrap()["path"],
            "/definitely/missing/spec.json"
        );
    }

    #[test]
    fn matching_sizes_pass_the_size_check() {
        let spec = parse(&spec_text(json!([image("a.png")])), file()).unwrap();
        let metadata = [ImageMetadata {
            name: "a.png".into(),
            width: 2,
            height: 2,
        }];

        assert!(check_sizes(&spec, &metadata).is_ok());
    }

    #[test]
    fn larger_input_passes_the_size_check_because_it_can_shrink() {
        let spec = parse(&spec_text(json!([image("a.png")])), file()).unwrap();
        let metadata = [ImageMetadata {
            name: "a.png".into(),
            width: 5,
            height: 2,
        }];

        assert!(check_sizes(&spec, &metadata).is_ok());
    }

    #[test]
    fn smaller_input_fails_the_size_check_with_details() {
        let spec = parse(&spec_text(json!([image("a.png")])), file()).unwrap();
        let metadata = [ImageMetadata {
            name: "a.png".into(),
            width: 1,
            height: 2,
        }];
        let error = check_sizes(&spec, &metadata).unwrap_err();

        assert_eq!(error.code(), "image_size_mismatch");
        assert_eq!(
            error.details(),
            Some(&json!({
                "index": 0, "path": "a.png", "width": 1, "height": 2,
                "expected": { "width": 2, "height": 2 }
            }))
        );
    }
}
