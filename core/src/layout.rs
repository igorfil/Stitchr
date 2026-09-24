use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    Grid { columns: usize },
    Stack { direction: StackDirection },
    Freeform { positions: Vec<(u32, u32)> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Start,
    Center,
    End,
}

impl Alignment {
    fn offset(self, space: u32) -> u32 {
        match self {
            Self::Start => 0,
            Self::Center => space / 2,
            Self::End => space,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeOptions {
    pub layout: Layout,
    pub spacing: u32,
    pub alignment: Alignment,
    pub max_size: Option<u32>,
}

impl Default for ComposeOptions {
    fn default() -> Self {
        Self {
            layout: Layout::Grid { columns: 2 },
            spacing: 0,
            alignment: Alignment::Start,
            max_size: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeError {
    NoImages,
    InvalidImageDimensions,
    InvalidColumns,
    InvalidMaxSize,
    FreeformPositionCount { expected: usize, actual: usize },
    SizeOverflow,
}

impl fmt::Display for ComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoImages => write!(f, "at least one image is required"),
            Self::InvalidImageDimensions => write!(f, "image dimensions must be nonzero"),
            Self::InvalidColumns => write!(f, "grid columns must be nonzero"),
            Self::InvalidMaxSize => write!(f, "max size must be nonzero"),
            Self::FreeformPositionCount { expected, actual } => {
                write!(f, "expected {expected} freeform positions, got {actual}")
            }
            Self::SizeOverflow => write!(f, "composition dimensions exceed the supported size"),
        }
    }
}

impl std::error::Error for ComposeError {}

pub(crate) struct Plan {
    pub positions: Vec<(u32, u32)>,
    pub width: u32,
    pub height: u32,
}

pub(crate) fn plan(sizes: &[(u32, u32)], options: &ComposeOptions) -> Result<Plan, ComposeError> {
    if sizes.is_empty() {
        return Err(ComposeError::NoImages);
    }
    if sizes
        .iter()
        .any(|&(width, height)| width == 0 || height == 0)
    {
        return Err(ComposeError::InvalidImageDimensions);
    }
    if options.max_size == Some(0) {
        return Err(ComposeError::InvalidMaxSize);
    }

    match &options.layout {
        Layout::Grid { columns: 0 } => Err(ComposeError::InvalidColumns),
        Layout::Grid { columns } => grid(sizes, (*columns).min(sizes.len()), options),
        Layout::Stack { direction } => stack(sizes, *direction, options),
        Layout::Freeform { positions } => freeform(sizes, positions),
    }
}

fn axis(lengths: &[u32], spacing: u32) -> Result<(Vec<u32>, u32), ComposeError> {
    let mut starts = Vec::with_capacity(lengths.len());
    let mut extent = 0_u32;
    for &length in lengths {
        if !starts.is_empty() {
            extent = extent
                .checked_add(spacing)
                .ok_or(ComposeError::SizeOverflow)?;
        }
        starts.push(extent);
        extent = extent
            .checked_add(length)
            .ok_or(ComposeError::SizeOverflow)?;
    }
    Ok((starts, extent))
}

fn grid(
    sizes: &[(u32, u32)],
    columns: usize,
    options: &ComposeOptions,
) -> Result<Plan, ComposeError> {
    let mut widths = vec![0; columns];
    let mut heights = vec![0; sizes.len().div_ceil(columns)];

    for (index, &(width, height)) in sizes.iter().enumerate() {
        widths[index % columns] = widths[index % columns].max(width);
        heights[index / columns] = heights[index / columns].max(height);
    }

    let (lefts, width) = axis(&widths, options.spacing)?;
    let (tops, height) = axis(&heights, options.spacing)?;
    let positions = sizes
        .iter()
        .enumerate()
        .map(|(index, &(image_width, image_height))| {
            let column = index % columns;
            let row = index / columns;
            (
                lefts[column] + options.alignment.offset(widths[column] - image_width),
                tops[row] + options.alignment.offset(heights[row] - image_height),
            )
        })
        .collect();

    Ok(Plan {
        positions,
        width,
        height,
    })
}

fn stack(
    sizes: &[(u32, u32)],
    direction: StackDirection,
    options: &ComposeOptions,
) -> Result<Plan, ComposeError> {
    match direction {
        StackDirection::Horizontal => {
            let widths: Vec<_> = sizes.iter().map(|&(width, _)| width).collect();
            let (lefts, width) = axis(&widths, options.spacing)?;
            let height = sizes.iter().map(|&(_, height)| height).max().unwrap_or(0);
            let positions = sizes
                .iter()
                .zip(lefts)
                .map(|(&(_, image_height), x)| (x, options.alignment.offset(height - image_height)))
                .collect();
            Ok(Plan {
                positions,
                width,
                height,
            })
        }
        StackDirection::Vertical => {
            let heights: Vec<_> = sizes.iter().map(|&(_, height)| height).collect();
            let (tops, height) = axis(&heights, options.spacing)?;
            let width = sizes.iter().map(|&(width, _)| width).max().unwrap_or(0);
            let positions = sizes
                .iter()
                .zip(tops)
                .map(|(&(image_width, _), y)| (options.alignment.offset(width - image_width), y))
                .collect();
            Ok(Plan {
                positions,
                width,
                height,
            })
        }
    }
}

fn freeform(sizes: &[(u32, u32)], positions: &[(u32, u32)]) -> Result<Plan, ComposeError> {
    if sizes.len() != positions.len() {
        return Err(ComposeError::FreeformPositionCount {
            expected: sizes.len(),
            actual: positions.len(),
        });
    }

    let (width, height) = sizes.iter().zip(positions).try_fold(
        (0_u32, 0_u32),
        |(width, height), (&(image_width, image_height), &(x, y))| {
            let right = x
                .checked_add(image_width)
                .ok_or(ComposeError::SizeOverflow)?;
            let bottom = y
                .checked_add(image_height)
                .ok_or(ComposeError::SizeOverflow)?;
            Ok::<_, ComposeError>((width.max(right), height.max(bottom)))
        },
    )?;

    Ok(Plan {
        positions: positions.to_vec(),
        width,
        height,
    })
}
