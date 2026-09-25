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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecPosition {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionSpec {
    pub paths: Vec<String>,
    pub layout: SpecLayout,
    pub background_color: RgbColor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementPlan {
    pub positions: Vec<(u32, u32)>,
    pub width: u32,
    pub height: u32,
    pub background_color: RgbColor,
}

pub fn plan_spec(
    spec: &CompositionSpec,
    dimensions: &[(u32, u32)],
) -> Result<PlacementPlan, ComposeError> {
    if spec.paths.len() != dimensions.len() {
        return Err(ComposeError::ImageCountMismatch {
            expected: spec.paths.len(),
            actual: dimensions.len(),
        });
    }

    let (layout, spacing, alignment) = match &spec.layout {
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
    };
    let plan = layout::plan(
        dimensions,
        &ComposeOptions {
            layout,
            spacing,
            alignment,
        },
    )?;
    Ok(PlacementPlan {
        positions: plan.positions,
        width: plan.width,
        height: plan.height,
        background_color: spec.background_color,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_spec() -> CompositionSpec {
        CompositionSpec {
            paths: vec![
                "one.png".into(),
                "two.png".into(),
                "three.png".into(),
                "four.png".into(),
            ],
            layout: SpecLayout::Grid {
                columns: 2,
                spacing: 1,
                alignment: Alignment::End,
            },
            background_color: RgbColor::default(),
        }
    }

    #[test]
    fn plans_grid_without_loading_or_painting_images() {
        let plan = plan_spec(&grid_spec(), &[(1, 2), (2, 1), (3, 2), (1, 3)]).unwrap();
        assert_eq!((plan.width, plan.height), (8, 8));
        assert_eq!(plan.positions, [(3, 1), (5, 2), (1, 5), (6, 4)]);
    }

    #[test]
    fn serialized_spec_is_stable_and_inspectable() {
        let spec = grid_spec();
        let value = serde_json::to_value(&spec).unwrap();
        assert_eq!(value["layout"]["type"], "grid");
        assert_eq!(value["layout"]["columns"], 2);
        assert_eq!(value["layout"]["spacing"], 1);
        assert_eq!(value["layout"]["alignment"], "end");
        assert_eq!(value["paths"][0], "one.png");
        assert_eq!(value["background_color"]["red"], 0);
        assert!(value.get("version").is_none());
        assert!(value.get("max_size").is_none());
        assert!(value.get("spacing").is_none());
        assert_eq!(
            serde_json::from_value::<CompositionSpec>(value).unwrap(),
            spec
        );
    }

    #[test]
    fn rejects_image_count_mismatch() {
        assert_eq!(
            plan_spec(&grid_spec(), &[(1, 1)]),
            Err(ComposeError::ImageCountMismatch {
                expected: 4,
                actual: 1
            })
        );
    }

    #[test]
    fn stack_and_freeform_have_distinct_plan_inputs() {
        let stack = CompositionSpec {
            paths: vec!["one.png".into(), "two.png".into()],
            layout: SpecLayout::Stack {
                direction: StackDirection::Horizontal,
                spacing: 2,
                alignment: Alignment::Center,
            },
            background_color: RgbColor::default(),
        };
        let stacked = plan_spec(&stack, &[(2, 1), (1, 3)]).unwrap();
        assert_eq!((stacked.width, stacked.height), (9, 7));
        assert_eq!(stacked.positions, [(2, 3), (6, 2)]);

        let freeform = CompositionSpec {
            paths: stack.paths,
            layout: SpecLayout::Freeform {
                positions: vec![SpecPosition { x: 1, y: 2 }, SpecPosition { x: 3, y: 0 }],
            },
            background_color: RgbColor::default(),
        };
        let placed = plan_spec(&freeform, &[(1, 1), (2, 2)]).unwrap();
        assert_eq!((placed.width, placed.height), (5, 3));
        assert_eq!(placed.positions, [(1, 2), (3, 0)]);
    }
}
