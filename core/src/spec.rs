use crate::{layout, Alignment, ComposeError, ComposeOptions, Layout, RgbColor, StackDirection};
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
    pub position: SpecPosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionParams {
    pub layout: SpecLayout,
    pub background_color: RgbColor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionSpec {
    pub images: Vec<ImagePlacement>,
    pub layout: SpecLayout,
    pub width: u32,
    pub height: u32,
    pub background_color: RgbColor,
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
    let dimensions = images
        .iter()
        .map(|image| (image.width, image.height))
        .collect::<Vec<_>>();
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
        &dimensions,
        &ComposeOptions {
            layout,
            spacing,
            alignment,
        },
    )?;
    let images = images
        .iter()
        .zip(plan.positions)
        .map(|(image, (x, y))| ImagePlacement {
            name: image.name.clone(),
            position: SpecPosition { x, y },
        })
        .collect();
    Ok(CompositionSpec {
        images,
        layout: params.layout.clone(),
        width: plan.width,
        height: plan.height,
        background_color: params.background_color,
    })
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
        assert!(value["images"][0].get("width").is_none());
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
