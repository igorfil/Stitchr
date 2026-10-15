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

    pub(crate) fn report(self, json_output: bool, exit_code: ExitCode) -> ExitCode {
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
        exit_code
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
            ComposeError::SizeOverflow => "canvas_too_large",
        };
        Self::new(code, error.to_string())
    }
}
