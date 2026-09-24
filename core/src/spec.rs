use crate::{layout, Alignment, ComposeError, ComposeOptions, Layout, StackDirection};
use serde::{Deserialize, Serialize};

pub const SPEC_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "snake_case")]
pub enum SpecLayout {
    Grid,
    Horizontal,
    Vertical,
    Freeform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "snake_case")]
pub enum SpecAlignment {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct SpecPosition {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct CompositionSpec {
    pub version: u32,
    pub paths: Vec<String>,
    pub layout: SpecLayout,
    pub columns: u32,
    pub spacing: u32,
    pub alignment: SpecAlignment,
    pub max_size: Option<u32>,
    pub positions: Vec<SpecPosition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementPlan {
    pub positions: Vec<(u32, u32)>,
    pub width: u32,
    pub height: u32,
    pub max_size: Option<u32>,
}

pub fn plan_spec(
    spec: &CompositionSpec,
    dimensions: &[(u32, u32)],
) -> Result<PlacementPlan, ComposeError> {
    if spec.version != SPEC_VERSION {
        return Err(ComposeError::UnsupportedSpecVersion {
            actual: spec.version,
        });
    }
    if spec.paths.len() != dimensions.len() {
        return Err(ComposeError::ImageCountMismatch {
            expected: spec.paths.len(),
            actual: dimensions.len(),
        });
    }

    let layout = match spec.layout {
        SpecLayout::Grid => Layout::Grid {
            columns: spec.columns as usize,
        },
        SpecLayout::Horizontal => Layout::Stack {
            direction: StackDirection::Horizontal,
        },
        SpecLayout::Vertical => Layout::Stack {
            direction: StackDirection::Vertical,
        },
        SpecLayout::Freeform => Layout::Freeform {
            positions: spec
                .positions
                .iter()
                .map(|point| (point.x, point.y))
                .collect(),
        },
    };
    let alignment = match spec.alignment {
        SpecAlignment::Start => Alignment::Start,
        SpecAlignment::Center => Alignment::Center,
        SpecAlignment::End => Alignment::End,
    };
    let plan = layout::plan(
        dimensions,
        &ComposeOptions {
            layout,
            spacing: spec.spacing,
            alignment,
            max_size: spec.max_size,
        },
    )?;
    Ok(PlacementPlan {
        positions: plan.positions,
        width: plan.width,
        height: plan.height,
        max_size: spec.max_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_spec() -> CompositionSpec {
        CompositionSpec {
            version: SPEC_VERSION,
            paths: vec![
                "one.png".into(),
                "two.png".into(),
                "three.png".into(),
                "four.png".into(),
            ],
            layout: SpecLayout::Grid,
            columns: 2,
            spacing: 1,
            alignment: SpecAlignment::End,
            max_size: None,
            positions: Vec::new(),
        }
    }

    #[test]
    fn plans_grid_without_loading_or_painting_images() {
        let plan = plan_spec(&grid_spec(), &[(1, 2), (2, 1), (3, 2), (1, 3)]).unwrap();
        assert_eq!((plan.width, plan.height), (6, 6));
        assert_eq!(plan.positions, [(2, 0), (4, 1), (0, 4), (5, 3)]);
    }

    #[test]
    fn serialized_spec_is_stable_and_inspectable() {
        let spec = grid_spec();
        let value = serde_json::to_value(&spec).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["layout"], "grid");
        assert_eq!(value["paths"][0], "one.png");
        assert_eq!(value["spacing"], 1);
        assert_eq!(
            serde_json::from_value::<CompositionSpec>(value).unwrap(),
            spec
        );
    }

    #[test]
    fn rejects_wrong_version_and_image_count() {
        let wrong_version = CompositionSpec {
            version: 2,
            ..grid_spec()
        };
        assert_eq!(
            plan_spec(&wrong_version, &[]),
            Err(ComposeError::UnsupportedSpecVersion { actual: 2 })
        );
        assert_eq!(
            plan_spec(&grid_spec(), &[(1, 1)]),
            Err(ComposeError::ImageCountMismatch {
                expected: 4,
                actual: 1
            })
        );
    }
}
