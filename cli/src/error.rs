use crate::SCHEMA_VERSION;
use serde_json::json;
use std::process::ExitCode;
use stitchr_core::ComposeError;

#[derive(Debug)]
pub(crate) struct CliError {
    code: &'static str,
    message: String,
}

impl CliError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    #[cfg(test)]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn exit_code(&self) -> u8 {
        if self.code == "invalid_arguments" {
            2
        } else {
            1
        }
    }

    pub(crate) fn report(self, json_output: bool) -> ExitCode {
        if json_output {
            println!(
                "{}",
                json!({
                    "schema_version": SCHEMA_VERSION,
                    "ok": false,
                    "error": { "code": self.code, "message": self.message }
                })
            );
        } else {
            eprintln!("stitchr: {}", self.message);
        }
        ExitCode::from(self.exit_code())
    }
}

impl From<ComposeError> for CliError {
    fn from(error: ComposeError) -> Self {
        let code = match error {
            ComposeError::NoImages => "no_images",
            ComposeError::InvalidImageDimensions => "invalid_image_dimensions",
            ComposeError::InvalidColumns => "invalid_columns",
            ComposeError::ImageCountMismatch { .. } => "image_count_mismatch",
            ComposeError::FreeformPositionCount { .. } => "freeform_position_count",
            ComposeError::SplitSizeMismatch => "split_size_mismatch",
            ComposeError::LabelCountMismatch { .. } => "label_count_mismatch",
            ComposeError::SizeOverflow => "canvas_too_large",
        };
        Self::new(code, error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            (ComposeError::SplitSizeMismatch, "split_size_mismatch"),
            (
                ComposeError::LabelCountMismatch {
                    expected: 1,
                    actual: 2,
                },
                "label_count_mismatch",
            ),
            (ComposeError::SizeOverflow, "canvas_too_large"),
        ] {
            let error = CliError::from(error);
            assert_eq!(error.code(), code);
            assert_eq!(error.exit_code(), 1, "{code}");
        }
    }
}
