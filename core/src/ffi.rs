use crate::{
    compose, export_image, load_image, Alignment, ComposeOptions, DynamicImage, ExportFormat,
    Layout, StackDirection,
};
use image::ImageFormat;
use std::io::Cursor;

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum AppLayout {
    Grid,
    Horizontal,
    Vertical,
    Freeform,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum AppAlignment {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum AppFormat {
    Png,
    Jpeg,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AppPosition {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AppOptions {
    pub layout: AppLayout,
    pub columns: u32,
    pub spacing: u32,
    pub alignment: AppAlignment,
    pub max_size: Option<u32>,
    pub positions: Vec<AppPosition>,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum AppError {
    #[error("cannot load {path}: {message}")]
    Input { path: String, message: String },
    #[error("cannot compose images: {message}")]
    Composition { message: String },
    #[error("cannot encode preview: {message}")]
    Encoding { message: String },
    #[error("cannot save {path}: {message}")]
    Output { path: String, message: String },
}

fn render(
    paths: &[String],
    options: &AppOptions,
    preview_limit: Option<u32>,
) -> Result<DynamicImage, AppError> {
    let images = paths
        .iter()
        .map(|path| {
            load_image(path).map_err(|error| AppError::Input {
                path: path.clone(),
                message: error.to_string(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let layout = match options.layout {
        AppLayout::Grid => Layout::Grid {
            columns: options.columns as usize,
        },
        AppLayout::Horizontal => Layout::Stack {
            direction: StackDirection::Horizontal,
        },
        AppLayout::Vertical => Layout::Stack {
            direction: StackDirection::Vertical,
        },
        AppLayout::Freeform => Layout::Freeform {
            positions: options
                .positions
                .iter()
                .map(|position| (position.x, position.y))
                .collect(),
        },
    };
    let alignment = match options.alignment {
        AppAlignment::Start => Alignment::Start,
        AppAlignment::Center => Alignment::Center,
        AppAlignment::End => Alignment::End,
    };
    let max_size = preview_limit.map_or(options.max_size, |limit| {
        Some(options.max_size.map_or(limit, |size| size.min(limit)))
    });
    compose(
        &images,
        &ComposeOptions {
            layout,
            spacing: options.spacing,
            alignment,
            max_size,
        },
    )
    .map_err(|error| AppError::Composition {
        message: error.to_string(),
    })
}

#[uniffi::export]
pub fn preview_png(
    paths: Vec<String>,
    options: AppOptions,
    preview_limit: u32,
) -> Result<Vec<u8>, AppError> {
    let image = render(&paths, &options, Some(preview_limit))?;
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|error| AppError::Encoding {
            message: error.to_string(),
        })?;
    Ok(bytes.into_inner())
}

#[uniffi::export]
pub fn export_file(
    paths: Vec<String>,
    options: AppOptions,
    destination: String,
    format: AppFormat,
) -> Result<(), AppError> {
    let image = render(&paths, &options, None)?;
    let format = match format {
        AppFormat::Png => ExportFormat::Png,
        AppFormat::Jpeg => ExportFormat::Jpeg,
    };
    export_image(&image, &destination, format).map_err(|error| AppError::Output {
        path: destination,
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> AppOptions {
        AppOptions {
            layout: AppLayout::Horizontal,
            columns: 2,
            spacing: 2,
            alignment: AppAlignment::Start,
            max_size: None,
            positions: Vec::new(),
        }
    }

    #[test]
    fn preview_caps_output_but_export_uses_original_size() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        let target = directory.path().join("result.png");
        export_image(&DynamicImage::new_rgba8(10, 5), &source, ExportFormat::Png).unwrap();
        let paths = vec![
            source.to_str().unwrap().to_string(),
            source.to_str().unwrap().to_string(),
        ];

        let preview = preview_png(paths.clone(), options(), 11).unwrap();
        let image = image::load_from_memory_with_format(&preview, ImageFormat::Png).unwrap();
        assert_eq!(image.width(), 11);

        export_file(
            paths,
            options(),
            target.to_str().unwrap().to_string(),
            AppFormat::Png,
        )
        .unwrap();
        let image = load_image(target).unwrap();
        assert_eq!((image.width(), image.height()), (26, 9));
    }

    #[test]
    fn reports_input_and_layout_errors() {
        let error =
            preview_png(vec!["/missing/stitchr.png".to_owned()], options(), 100).unwrap_err();
        assert!(matches!(error, AppError::Input { .. }));

        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        export_image(&DynamicImage::new_rgba8(1, 1), &source, ExportFormat::Png).unwrap();
        let invalid = AppOptions {
            columns: 0,
            layout: AppLayout::Grid,
            ..options()
        };
        let error =
            preview_png(vec![source.to_str().unwrap().to_owned()], invalid, 100).unwrap_err();
        assert!(matches!(error, AppError::Composition { .. }));
    }
}
