use crate::{
    compose_spec, export_image, load_image_with_metadata, render_spec, Alignment,
    CompositionParams, DynamicImage, ExportFormat, RgbColor, SpecLayout, SpecPosition,
    StackDirection,
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

fn render(paths: &[String], options: &AppOptions) -> Result<DynamicImage, AppError> {
    let loaded = paths
        .iter()
        .map(|path| {
            load_image_with_metadata(path).map_err(|error| AppError::Input {
                path: path.clone(),
                message: error.to_string(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let metadata = loaded
        .iter()
        .map(|image| image.metadata.clone())
        .collect::<Vec<_>>();
    let alignment = match options.alignment {
        AppAlignment::Start => Alignment::Start,
        AppAlignment::Center => Alignment::Center,
        AppAlignment::End => Alignment::End,
    };
    let layout = match options.layout {
        AppLayout::Grid => SpecLayout::Grid {
            columns: options.columns,
            spacing: options.spacing,
            alignment,
        },
        AppLayout::Horizontal => SpecLayout::Stack {
            direction: StackDirection::Horizontal,
            spacing: options.spacing,
            alignment,
        },
        AppLayout::Vertical => SpecLayout::Stack {
            direction: StackDirection::Vertical,
            spacing: options.spacing,
            alignment,
        },
        AppLayout::Freeform => SpecLayout::Freeform {
            positions: options
                .positions
                .iter()
                .map(|position| SpecPosition {
                    x: position.x,
                    y: position.y,
                })
                .collect(),
        },
    };
    let spec = compose_spec(
        &metadata,
        &CompositionParams {
            layout,
            background_color: RgbColor::default(),
        },
    )
    .map_err(|error| AppError::Composition {
        message: error.to_string(),
    })?;
    let images = loaded
        .into_iter()
        .map(|loaded| loaded.image)
        .collect::<Vec<_>>();
    render_spec(&spec, &images).map_err(|error| AppError::Composition {
        message: error.to_string(),
    })
}

#[uniffi::export]
pub fn preview_png(paths: Vec<String>, options: AppOptions) -> Result<Vec<u8>, AppError> {
    let image = render(&paths, &options)?;
    let image = if image.width().max(image.height()) > 1_200 {
        image.resize(1_200, 1_200, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
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
    let image = render(&paths, &options)?;
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
    use crate::load_image;

    fn options() -> AppOptions {
        AppOptions {
            layout: AppLayout::Horizontal,
            columns: 2,
            spacing: 2,
            alignment: AppAlignment::Start,
            positions: Vec::new(),
        }
    }

    #[test]
    fn preview_is_display_scaled_but_export_uses_original_size() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        let target = directory.path().join("result.png");
        export_image(&DynamicImage::new_rgba8(700, 5), &source, ExportFormat::Png).unwrap();
        let paths = vec![
            source.to_str().unwrap().to_string(),
            source.to_str().unwrap().to_string(),
        ];

        let preview = preview_png(paths.clone(), options()).unwrap();
        let image = image::load_from_memory_with_format(&preview, ImageFormat::Png).unwrap();
        assert_eq!(image.width(), 1_200);

        export_file(
            paths,
            options(),
            target.to_str().unwrap().to_string(),
            AppFormat::Png,
        )
        .unwrap();
        let image = load_image(target).unwrap();
        assert_eq!((image.width(), image.height()), (1_406, 9));
    }

    #[test]
    fn reports_input_and_layout_errors() {
        let error = preview_png(vec!["/missing/stitchr.png".to_owned()], options()).unwrap_err();
        assert!(matches!(error, AppError::Input { .. }));

        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        export_image(&DynamicImage::new_rgba8(1, 1), &source, ExportFormat::Png).unwrap();
        let invalid = AppOptions {
            columns: 0,
            layout: AppLayout::Grid,
            ..options()
        };
        let error = preview_png(vec![source.to_str().unwrap().to_owned()], invalid).unwrap_err();
        assert!(matches!(error, AppError::Composition { .. }));
    }
}
