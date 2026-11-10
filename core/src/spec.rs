use crate::{
    annotation::validate_annotations, fit::PlacementFit, frame, layout, Alignment, ComposeError,
    ComposeOptions, Layout, RgbColor, SpecAnnotation, SpecFrame, SpecGradient, StackDirection,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SpecLayout {
    Grid {
        columns: u32,
        spacing: u32,
        alignment: Alignment,
    },
    Stack {
        direction: StackDirection,
        spacing: u32,
        alignment: Alignment,
    },
    Freeform {
        positions: Vec<SpecPosition>,
    },
    Split {
        divider: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecPosition {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageMetadata {
    pub name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagePlacement {
    pub name: String,
    /// Where the image was loaded from, when known; used to render a spec read back from JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Size the image is drawn at, in pixels.
    pub width: u32,
    pub height: u32,
    /// How the source is fitted when its aspect ratio differs; `None` scales it directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<PlacementFit>,
    pub position: SpecPosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionParams {
    pub layout: SpecLayout,
    pub background_color: RgbColor,
    pub background_gradient: Option<SpecGradient>,
    pub frame: Option<SpecFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionSpec {
    pub images: Vec<ImagePlacement>,
    pub layout: SpecLayout,
    pub width: u32,
    pub height: u32,
    pub background_color: RgbColor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_gradient: Option<SpecGradient>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<SpecFrame>,
    /// Drawn on top of everything else, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<SpecAnnotation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementPlan {
    pub positions: Vec<(u32, u32)>,
    pub width: u32,
    pub height: u32,
    pub background_color: RgbColor,
}

pub fn compose_spec(
    images: &[ImageMetadata],
    params: &CompositionParams,
) -> Result<CompositionSpec, ComposeError> {
    if let Some(frame) = &params.frame {
        if frame.labels.len() != images.len() {
            return Err(ComposeError::LabelCountMismatch {
                expected: images.len(),
                actual: frame.labels.len(),
            });
        }
    }
    let split = matches!(params.layout, SpecLayout::Split { .. });
    let dimensions = images
        .iter()
        .map(|image| (image.width, image.height))
        .collect::<Vec<_>>();
    let tiles = match &params.frame {
        Some(frame) if !split => dimensions
            .iter()
            .map(|&size| frame::tile_size(frame, size))
            .collect::<Result<Vec<_>, _>>()?,
        _ => dimensions,
    };
    let (layout, spacing, alignment) = match &params.layout {
        SpecLayout::Grid {
            columns,
            spacing,
            alignment,
        } => (
            Layout::Grid {
                columns: *columns as usize,
            },
            *spacing,
            *alignment,
        ),
        SpecLayout::Stack {
            direction,
            spacing,
            alignment,
        } => (
            Layout::Stack {
                direction: *direction,
            },
            *spacing,
            *alignment,
        ),
        SpecLayout::Freeform { positions } => (
            Layout::Freeform {
                positions: positions.iter().map(|point| (point.x, point.y)).collect(),
            },
            0,
            Alignment::Start,
        ),
        SpecLayout::Split { divider } => (Layout::Split { divider: *divider }, 0, Alignment::Start),
    };
    let plan = layout::plan(
        &tiles,
        &ComposeOptions {
            layout,
            spacing,
            alignment,
        },
    )?;
    let (left, top) = params.frame.as_ref().map_or((0, 0), frame::inset);
    let (width, height) = match &params.frame {
        Some(frame) if split => frame::tile_size(frame, (plan.width, plan.height))?,
        _ => (plan.width, plan.height),
    };
    let images = images
        .iter()
        .zip(plan.positions)
        .map(|(image, (x, y))| ImagePlacement {
            name: image.name.clone(),
            path: None,
            width: image.width,
            height: image.height,
            fit: None,
            position: SpecPosition {
                x: x + left,
                y: y + top,
            },
        })
        .collect();
    Ok(CompositionSpec {
        images,
        layout: params.layout.clone(),
        width,
        height,
        background_color: params.background_color,
        background_gradient: params.background_gradient,
        frame: params.frame.clone(),
        annotations: Vec::new(),
    })
}

/// Checks that a spec, for example one read back from JSON, is internally consistent.
pub fn validate_spec(spec: &CompositionSpec) -> Result<(), ComposeError> {
    if spec.images.is_empty() {
        return Err(ComposeError::NoImages);
    }
    if spec.width == 0
        || spec.height == 0
        || spec
            .images
            .iter()
            .any(|image| image.width == 0 || image.height == 0)
    {
        return Err(ComposeError::InvalidImageDimensions);
    }
    if let Some(frame) = &spec.frame {
        if frame.labels.len() != spec.images.len() {
            return Err(ComposeError::LabelCountMismatch {
                expected: spec.images.len(),
                actual: frame.labels.len(),
            });
        }
    }
    if let SpecLayout::Split { .. } = spec.layout {
        let (left, top) = spec.frame.as_ref().map_or((0, 0), frame::inset);
        let size = (
            spec.width.saturating_sub(left.saturating_mul(2)),
            spec.height.saturating_sub(top.saturating_add(left)),
        );
        layout::require_same_size(
            spec.images.iter().map(|image| (image.width, image.height)),
            size,
        )?;
    }
    validate_annotations(&spec.annotations)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(name: &str, width: u32, height: u32) -> ImageMetadata {
        ImageMetadata {
            name: name.into(),
            width,
            height,
        }
    }

    #[test]
    fn composes_grid_placements_from_image_metadata() {
        let images = [
            metadata("one.png", 1, 2),
            metadata("two.png", 2, 1),
            metadata("three.png", 3, 2),
            metadata("four.png", 1, 3),
        ];
        let params = CompositionParams {
            layout: SpecLayout::Grid {
                columns: 2,
                spacing: 1,
                alignment: Alignment::End,
            },
            background_color: RgbColor::default(),
            background_gradient: None,
            frame: None,
        };

        let spec = compose_spec(&images, &params).unwrap();

        assert_eq!((spec.width, spec.height), (8, 8));
        assert_eq!(
            spec.images
                .iter()
                .map(|image| (image.position.x, image.position.y))
                .collect::<Vec<_>>(),
            [(3, 1), (5, 2), (1, 5), (6, 4)]
        );
    }

    #[test]
    fn serialized_spec_is_stable_and_inspectable() {
        let images = [metadata("one.png", 1, 2), metadata("two.png", 2, 1)];
        let spec = compose_spec(
            &images,
            &CompositionParams {
                layout: SpecLayout::Grid {
                    columns: 2,
                    spacing: 1,
                    alignment: Alignment::End,
                },
                background_color: RgbColor::default(),
                background_gradient: None,
                frame: None,
            },
        )
        .unwrap();
        let value = serde_json::to_value(&spec).unwrap();
        assert_eq!(value["layout"]["type"], "grid");
        assert_eq!(value["layout"]["columns"], 2);
        assert_eq!(value["layout"]["spacing"], 1);
        assert_eq!(value["layout"]["alignment"], "end");
        assert_eq!(value["images"][0]["name"], "one.png");
        assert_eq!(value["images"][0]["position"]["x"], 1);
        assert_eq!(value["width"], 6);
        assert_eq!(value["height"], 4);
        assert_eq!(value["background_color"]["red"], 0);
        assert!(value.get("version").is_none());
        assert!(value.get("max_size").is_none());
        assert!(value.get("paths").is_none());
        assert!(value.get("sizes").is_none());
        assert!(value.get("spacing").is_none());
        assert_eq!(value["images"][0]["width"], 1);
        assert_eq!(value["images"][0]["height"], 2);
        assert!(value["images"][0].get("path").is_none());
        assert_eq!(
            serde_json::from_value::<CompositionSpec>(value).unwrap(),
            spec
        );
    }

    #[test]
    fn composes_split_spec_with_all_images_at_origin() {
        let images = [metadata("light.png", 4, 3), metadata("dark.png", 4, 3)];
        let spec = compose_spec(
            &images,
            &CompositionParams {
                layout: SpecLayout::Split { divider: 2 },
                background_color: RgbColor::default(),
                background_gradient: None,
                frame: None,
            },
        )
        .unwrap();

        assert_eq!((spec.width, spec.height), (4, 3));
        let value = serde_json::to_value(&spec).unwrap();
        assert_eq!(value["layout"]["type"], "split");
        assert_eq!(value["layout"]["divider"], 2);
        assert_eq!(value["images"][1]["name"], "dark.png");
        assert_eq!(value["images"][1]["position"]["x"], 0);
        assert_eq!(value["images"][1]["position"]["y"], 0);
        assert_eq!(
            serde_json::from_value::<CompositionSpec>(value).unwrap(),
            spec
        );
    }

    #[test]
    fn rejects_freeform_position_count_mismatch() {
        let images = [metadata("one.png", 1, 1), metadata("two.png", 1, 1)];
        assert_eq!(
            compose_spec(
                &images,
                &CompositionParams {
                    layout: SpecLayout::Freeform {
                        positions: vec![SpecPosition { x: 0, y: 0 }],
                    },
                    background_color: RgbColor::default(),
                    background_gradient: None,
                    frame: None,
                }
            ),
            Err(ComposeError::FreeformPositionCount {
                expected: 2,
                actual: 1,
            })
        );
    }

    #[test]
    fn composes_stacks_and_freeform_from_image_metadata() {
        let images = [metadata("one.png", 2, 1), metadata("two.png", 1, 3)];
        let stacked = compose_spec(
            &images,
            &CompositionParams {
                layout: SpecLayout::Stack {
                    direction: StackDirection::Horizontal,
                    spacing: 2,
                    alignment: Alignment::Center,
                },
                background_color: RgbColor::default(),
                background_gradient: None,
                frame: None,
            },
        )
        .unwrap();
        assert_eq!((stacked.width, stacked.height), (9, 7));
        assert_eq!(
            stacked
                .images
                .iter()
                .map(|image| (image.position.x, image.position.y))
                .collect::<Vec<_>>(),
            [(2, 3), (6, 2)]
        );

        let freeform_images = [metadata("one.png", 1, 1), metadata("two.png", 2, 2)];
        let placed = compose_spec(
            &freeform_images,
            &CompositionParams {
                layout: SpecLayout::Freeform {
                    positions: vec![SpecPosition { x: 1, y: 2 }, SpecPosition { x: 3, y: 0 }],
                },
                background_color: RgbColor::default(),
                background_gradient: None,
                frame: None,
            },
        )
        .unwrap();
        assert_eq!((placed.width, placed.height), (5, 3));
        assert_eq!(
            placed
                .images
                .iter()
                .map(|image| (image.position.x, image.position.y))
                .collect::<Vec<_>>(),
            [(1, 2), (3, 0)]
        );
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    fn placement(width: u32, height: u32) -> ImagePlacement {
        ImagePlacement {
            name: "image.png".into(),
            path: None,
            width,
            height,
            fit: None,
            position: SpecPosition { x: 0, y: 0 },
        }
    }

    fn spec(layout: SpecLayout, images: Vec<ImagePlacement>, size: (u32, u32)) -> CompositionSpec {
        CompositionSpec {
            images,
            layout,
            width: size.0,
            height: size.1,
            background_color: RgbColor::default(),
            background_gradient: None,
            frame: None,
            annotations: Vec::new(),
        }
    }

    fn grid() -> SpecLayout {
        SpecLayout::Grid {
            columns: 2,
            spacing: 0,
            alignment: Alignment::Start,
        }
    }

    fn frame(labels: usize) -> SpecFrame {
        SpecFrame {
            labels: vec![String::new(); labels],
            border_width: 3,
            border_color: RgbColor::default(),
            label_size: 20,
            label_position: crate::LabelPosition::Inside,
            label_color: None,
            font: None,
            font_data: None,
        }
    }

    #[test]
    fn composed_specs_are_valid() {
        let images = [
            ImageMetadata {
                name: "a.png".into(),
                width: 3,
                height: 2,
            },
            ImageMetadata {
                name: "b.png".into(),
                width: 3,
                height: 2,
            },
        ];
        for layout in [grid(), SpecLayout::Split { divider: 1 }] {
            for frame in [None, Some(frame(2))] {
                let composed = compose_spec(
                    &images,
                    &CompositionParams {
                        layout: layout.clone(),
                        background_color: RgbColor::default(),
                        background_gradient: None,
                        frame,
                    },
                )
                .unwrap();

                assert_eq!(validate_spec(&composed), Ok(()), "{composed:?}");
            }
        }
    }

    #[test]
    fn spec_without_images_is_invalid() {
        assert_eq!(
            validate_spec(&spec(grid(), vec![], (1, 1))),
            Err(ComposeError::NoImages)
        );
    }

    #[test]
    fn zero_sized_canvas_or_image_is_invalid() {
        for (image, canvas) in [
            ((0, 1), (1, 1)),
            ((1, 0), (1, 1)),
            ((1, 1), (0, 1)),
            ((1, 1), (1, 0)),
        ] {
            assert_eq!(
                validate_spec(&spec(grid(), vec![placement(image.0, image.1)], canvas)),
                Err(ComposeError::InvalidImageDimensions),
                "{image:?} {canvas:?}"
            );
        }
    }

    #[test]
    fn label_count_must_match_image_count() {
        let mut framed = spec(grid(), vec![placement(1, 1), placement(1, 1)], (10, 10));
        framed.frame = Some(frame(1));

        assert_eq!(
            validate_spec(&framed),
            Err(ComposeError::LabelCountMismatch {
                expected: 2,
                actual: 1
            })
        );
    }

    #[test]
    fn split_images_must_match_the_canvas() {
        let mismatched = spec(
            SpecLayout::Split { divider: 0 },
            vec![placement(4, 4), placement(4, 5)],
            (4, 4),
        );

        assert_eq!(
            validate_spec(&mismatched),
            Err(ComposeError::SplitSizeMismatch {
                index: 1,
                expected: (4, 4),
                actual: (4, 5)
            })
        );
    }

    #[test]
    fn framed_split_images_match_the_canvas_inside_the_border() {
        let mut framed = spec(
            SpecLayout::Split { divider: 0 },
            vec![placement(4, 4), placement(4, 4)],
            (10, 10),
        );
        framed.frame = Some(frame(2));

        assert_eq!(validate_spec(&framed), Ok(()));
        framed.width = 11;
        assert!(matches!(
            validate_spec(&framed),
            Err(ComposeError::SplitSizeMismatch { index: 0, .. })
        ));
    }

    #[test]
    fn non_split_placements_may_lie_anywhere() {
        let mut placed = placement(5, 5);
        placed.position = SpecPosition { x: 100, y: 100 };

        assert_eq!(validate_spec(&spec(grid(), vec![placed], (2, 2))), Ok(()));
    }

    #[test]
    fn path_round_trips_and_is_omitted_when_unknown() {
        let mut with_path = placement(1, 1);
        with_path.path = Some("/tmp/a.png".into());

        let value = serde_json::to_value(&with_path).unwrap();

        assert_eq!(value["path"], "/tmp/a.png");
        assert_eq!(
            serde_json::from_value::<ImagePlacement>(value).unwrap(),
            with_path
        );
        assert!(serde_json::to_value(placement(1, 1))
            .unwrap()
            .get("path")
            .is_none());
    }

    #[test]
    fn placement_without_a_size_does_not_deserialize() {
        let value = serde_json::json!({ "name": "a.png", "position": { "x": 0, "y": 0 } });

        assert!(serde_json::from_value::<ImagePlacement>(value).is_err());
    }
}
