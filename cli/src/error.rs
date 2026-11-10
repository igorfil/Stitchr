use crate::{warnings::Warning, SCHEMA_VERSION};
use clap::error::{ContextKind, ContextValue};
use serde_json::{json, Value};
use std::{path::Path, process::ExitCode};
use stitchr_core::ComposeError;

#[derive(Debug)]
pub(crate) struct CliError {
    code: &'static str,
    message: String,
    details: Option<Value>,
}

impl CliError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub(crate) fn with_details(self, details: Value) -> Self {
        Self {
            details: Some(details),
            ..self
        }
    }

    /// Adds the input path to errors that point at an input by index.
    pub(crate) fn with_input_paths(self, paths: &[impl AsRef<Path>]) -> Self {
        let Some(mut details) = self.details.clone() else {
            return self;
        };
        let Some(path) = details["index"]
            .as_u64()
            .and_then(|index| paths.get(usize::try_from(index).ok()?))
        else {
            return self;
        };
        details["path"] = json!(path.as_ref().to_string_lossy());
        self.with_details(details)
    }

    #[cfg(test)]
    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    #[cfg(test)]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    #[cfg(test)]
    pub(crate) fn details(&self) -> Option<&Value> {
        self.details.as_ref()
    }

    pub(crate) fn exit_code(&self) -> u8 {
        if self.code == "invalid_arguments" {
            2
        } else {
            1
        }
    }

    fn json(&self) -> Value {
        let mut error = json!({ "code": self.code, "message": self.message });
        if let Some(details) = &self.details {
            error["details"] = details.clone();
        }
        json!({ "schema_version": SCHEMA_VERSION, "ok": false, "error": error })
    }

    pub(crate) fn report(self, json_output: bool) -> ExitCode {
        if json_output {
            println!("{}", self.json());
        } else {
            eprintln!("stitchr: {}", self.message);
        }
        ExitCode::from(self.exit_code())
    }

    pub(crate) fn strict(warnings: &[Warning]) -> Self {
        let reasons = warnings
            .iter()
            .map(|warning| warning.reason.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        Self::new(
            "invalid_arguments",
            format!("--strict treats warnings as errors: {reasons}"),
        )
        .with_details(json!({
            "kind": "strict",
            "warnings": warnings.iter().map(Warning::json).collect::<Vec<_>>()
        }))
    }
}

fn snake_case(name: &str) -> String {
    name.chars()
        .enumerate()
        .flat_map(|(index, character)| {
            let separator = (index > 0 && character.is_ascii_uppercase()).then_some('_');
            separator
                .into_iter()
                .chain(std::iter::once(character.to_ascii_lowercase()))
        })
        .collect()
}

fn context_string(error: &clap::Error, kind: ContextKind) -> Option<String> {
    match error.get(kind)? {
        ContextValue::String(value) => Some(value.clone()),
        ContextValue::Strings(values) => values.first().cloned(),
        ContextValue::StyledStr(value) => Some(value.to_string()),
        _ => None,
    }
}

impl From<&clap::Error> for CliError {
    fn from(error: &clap::Error) -> Self {
        let mut details = json!({ "kind": snake_case(&format!("{:?}", error.kind())) });
        if let Some(option) = context_string(error, ContextKind::InvalidArg) {
            let name = option
                .split_whitespace()
                .next()
                .unwrap_or(&option)
                .to_owned();
            details["option"] = json!(name);
        }
        if let Some(value) = context_string(error, ContextKind::InvalidValue) {
            details["value"] = json!(value);
        }
        if let Some(ContextValue::Strings(allowed)) = error.get(ContextKind::ValidValue) {
            details["allowed"] = json!(allowed);
        }
        if let Some(suggestion) = context_string(error, ContextKind::SuggestedArg)
            .or_else(|| context_string(error, ContextKind::SuggestedValue))
        {
            details["suggestion"] = json!(suggestion);
        }
        if let Some(reason) = std::error::Error::source(error) {
            details["reason"] = json!(reason.to_string());
        }
        Self::new("invalid_arguments", error.to_string()).with_details(details)
    }
}

fn size(width: u32, height: u32) -> Value {
    json!({ "width": width, "height": height })
}

impl From<ComposeError> for CliError {
    fn from(error: ComposeError) -> Self {
        let (code, details) = match &error {
            ComposeError::NoImages => ("no_images", None),
            ComposeError::InvalidImageDimensions => ("invalid_image_dimensions", None),
            ComposeError::InvalidColumns => ("invalid_columns", None),
            ComposeError::ImageCountMismatch { expected, actual } => (
                "image_count_mismatch",
                Some(json!({ "expected": expected, "actual": actual })),
            ),
            ComposeError::FreeformPositionCount { expected, actual } => (
                "freeform_position_count",
                Some(json!({ "expected": expected, "actual": actual })),
            ),
            ComposeError::SplitSizeMismatch {
                index,
                expected,
                actual,
            } => (
                "split_size_mismatch",
                Some(json!({
                    "index": index,
                    "width": actual.0,
                    "height": actual.1,
                    "expected": size(expected.0, expected.1)
                })),
            ),
            ComposeError::LabelCountMismatch { expected, actual } => (
                "label_count_mismatch",
                Some(json!({ "expected": expected, "actual": actual })),
            ),
            ComposeError::SizeOverflow => (
                "canvas_too_large",
                Some(json!({ "max_dimension": u32::MAX })),
            ),
            ComposeError::CanvasTooLarge {
                width,
                height,
                bytes,
                limit,
            } => (
                "canvas_too_large",
                Some(json!({
                    "width": width,
                    "height": height,
                    "bytes": bytes,
                    "limit_bytes": limit
                })),
            ),
        };
        let cli_error = Self::new(code, error.to_string());
        match details {
            Some(details) => cli_error.with_details(details),
            None => cli_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use clap::Parser;

    fn clap_error(args: &[&str]) -> CliError {
        let error = Cli::try_parse_from(std::iter::once("stitchr").chain(args.iter().copied()))
            .unwrap_err();
        CliError::from(&error)
    }

    #[test]
    fn argument_errors_exit_with_two() {
        assert_eq!(CliError::new("invalid_arguments", "bad").exit_code(), 2);
    }

    #[test]
    fn processing_errors_exit_with_one() {
        for code in [
            "input_error",
            "output_error",
            "unsupported_output_format",
            "canvas_too_large",
            "serialization_error",
        ] {
            assert_eq!(CliError::new(code, "failed").exit_code(), 1, "{code}");
        }
    }

    #[test]
    fn compose_errors_map_to_stable_codes() {
        for (error, code) in [
            (ComposeError::NoImages, "no_images"),
            (
                ComposeError::InvalidImageDimensions,
                "invalid_image_dimensions",
            ),
            (ComposeError::InvalidColumns, "invalid_columns"),
            (
                ComposeError::ImageCountMismatch {
                    expected: 1,
                    actual: 2,
                },
                "image_count_mismatch",
            ),
            (
                ComposeError::FreeformPositionCount {
                    expected: 1,
                    actual: 2,
                },
                "freeform_position_count",
            ),
            (
                ComposeError::SplitSizeMismatch {
                    index: 1,
                    expected: (1, 1),
                    actual: (2, 2),
                },
                "split_size_mismatch",
            ),
            (
                ComposeError::LabelCountMismatch {
                    expected: 1,
                    actual: 2,
                },
                "label_count_mismatch",
            ),
            (ComposeError::SizeOverflow, "canvas_too_large"),
            (
                ComposeError::CanvasTooLarge {
                    width: 1,
                    height: 1,
                    bytes: 4,
                    limit: 1,
                },
                "canvas_too_large",
            ),
        ] {
            let error = CliError::from(error);
            assert_eq!(error.code(), code);
            assert_eq!(error.exit_code(), 1, "{code}");
        }
    }

    #[test]
    fn errors_without_details_omit_the_field() {
        let value = CliError::new("input_error", "cannot load").json();

        assert_eq!(
            value,
            json!({
                "schema_version": SCHEMA_VERSION,
                "ok": false,
                "error": { "code": "input_error", "message": "cannot load" }
            })
        );
    }

    #[test]
    fn errors_with_details_nest_them_under_the_error() {
        let value = CliError::new("output_error", "cannot write")
            .with_details(json!({ "path": "out.png" }))
            .json();

        assert_eq!(value["error"]["details"], json!({ "path": "out.png" }));
    }

    #[test]
    fn split_mismatch_details_include_actual_and_expected_sizes() {
        let error = CliError::from(ComposeError::SplitSizeMismatch {
            index: 1,
            expected: (1280, 800),
            actual: (1024, 768),
        });

        assert_eq!(
            error.details(),
            Some(&json!({
                "index": 1, "width": 1024, "height": 768,
                "expected": { "width": 1280, "height": 800 }
            }))
        );
    }

    #[test]
    fn input_paths_are_added_for_indexed_errors() {
        let error = CliError::from(ComposeError::SplitSizeMismatch {
            index: 1,
            expected: (2, 2),
            actual: (3, 3),
        })
        .with_input_paths(&["shot1.png", "shot3.png"]);

        assert_eq!(error.details().unwrap()["path"], "shot3.png");
    }

    #[test]
    fn input_paths_leave_unindexed_errors_untouched() {
        let error = CliError::from(ComposeError::LabelCountMismatch {
            expected: 2,
            actual: 1,
        })
        .with_input_paths(&["a.png"]);

        assert_eq!(
            error.details(),
            Some(&json!({ "expected": 2, "actual": 1 }))
        );
        assert!(CliError::new("input_error", "x")
            .with_input_paths(&["a.png"])
            .details()
            .is_none());
    }

    #[test]
    fn input_paths_ignore_out_of_range_indexes() {
        let error = CliError::new("split_size_mismatch", "x")
            .with_details(json!({ "index": 5 }))
            .with_input_paths(&["a.png"]);

        assert_eq!(error.details(), Some(&json!({ "index": 5 })));
    }

    #[test]
    fn canvas_errors_report_computed_size_and_limit() {
        assert_eq!(
            CliError::from(ComposeError::CanvasTooLarge {
                width: 20_000,
                height: 10_000,
                bytes: 800_000_000,
                limit: 536_870_912,
            })
            .details(),
            Some(&json!({
                "width": 20_000, "height": 10_000,
                "bytes": 800_000_000, "limit_bytes": 536_870_912
            }))
        );
        assert_eq!(
            CliError::from(ComposeError::SizeOverflow).details(),
            Some(&json!({ "max_dimension": 4_294_967_295_u32 }))
        );
    }

    #[test]
    fn count_mismatches_report_expected_and_actual() {
        for error in [
            ComposeError::FreeformPositionCount {
                expected: 3,
                actual: 1,
            },
            ComposeError::LabelCountMismatch {
                expected: 3,
                actual: 1,
            },
            ComposeError::ImageCountMismatch {
                expected: 3,
                actual: 1,
            },
        ] {
            assert_eq!(
                CliError::from(error).details(),
                Some(&json!({ "expected": 3, "actual": 1 }))
            );
        }
    }

    #[test]
    fn snake_case_converts_clap_kind_names() {
        assert_eq!(snake_case("InvalidValue"), "invalid_value");
        assert_eq!(snake_case("UnknownArgument"), "unknown_argument");
        assert_eq!(snake_case("Io"), "io");
    }

    #[test]
    fn range_validation_reports_option_value_and_reason() {
        let error = clap_error(&["a.png", "--columns", "0"]);
        let details = error.details().unwrap();

        assert_eq!(details["kind"], "value_validation");
        assert_eq!(details["option"], "--columns");
        assert_eq!(details["value"], "0");
        assert!(details["reason"].as_str().unwrap().contains("1.."));
    }

    #[test]
    fn negative_value_reports_the_custom_reason() {
        let details = clap_error(&["a.png", "--spacing", "-5"])
            .details()
            .cloned()
            .unwrap();

        assert_eq!(
            details,
            json!({
                "kind": "value_validation", "option": "--spacing",
                "value": "-5", "reason": "must be ≥ 0"
            })
        );
    }

    #[test]
    fn enum_errors_list_allowed_values() {
        let details = clap_error(&["a.png", "--layout", "gird"])
            .details()
            .cloned()
            .unwrap();

        assert_eq!(details["kind"], "invalid_value");
        assert_eq!(details["option"], "--layout");
        assert_eq!(details["value"], "gird");
        assert_eq!(
            details["allowed"],
            json!(["grid", "horizontal", "vertical", "freeform", "split"])
        );
        assert_eq!(details["suggestion"], "grid");
    }

    #[test]
    fn unknown_flags_suggest_the_closest_option() {
        let details = clap_error(&["a.png", "--colums", "2"])
            .details()
            .cloned()
            .unwrap();

        assert_eq!(details["kind"], "unknown_argument");
        assert_eq!(details["option"], "--colums");
        assert_eq!(details["suggestion"], "--columns");
    }

    #[test]
    fn conflicts_name_the_offending_option() {
        let details = clap_error(&[
            "a.png",
            "--background",
            "red",
            "--background-gradient",
            "red",
            "blue",
        ])
        .details()
        .cloned()
        .unwrap();

        assert_eq!(details["kind"], "argument_conflict");
        assert!(details["option"]
            .as_str()
            .unwrap()
            .starts_with("--background"));
    }

    #[test]
    fn missing_inputs_have_a_kind() {
        let details = clap_error(&[]).details().cloned().unwrap();

        assert_eq!(details["kind"], "missing_required_argument");
    }

    #[test]
    fn strict_errors_list_the_warnings() {
        let warnings = crate::warnings::option_warnings(
            &crate::parse_cli(
                ["stitchr", "a.png", "--divider", "1"]
                    .map(std::ffi::OsString::from)
                    .to_vec(),
            )
            .unwrap(),
        );
        let error = CliError::strict(&warnings);

        assert_eq!(error.code(), "invalid_arguments");
        assert_eq!(
            error.details(),
            Some(&json!({
                "kind": "strict",
                "warnings": [{
                    "code": "option_ignored",
                    "option": "--divider",
                    "reason": "--divider is not used by the grid layout"
                }]
            }))
        );
    }
}
