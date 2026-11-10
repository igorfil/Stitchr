use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    Grid { columns: usize },
    Stack { direction: StackDirection },
    Freeform { positions: Vec<(u32, u32)> },
    Split { divider: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StackDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
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
}

impl Default for ComposeOptions {
    fn default() -> Self {
        Self {
            layout: Layout::Grid { columns: 2 },
            spacing: 0,
            alignment: Alignment::Start,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeError {
    NoImages,
    InvalidImageDimensions,
    InvalidColumns,
    ImageCountMismatch {
        expected: usize,
        actual: usize,
    },
    /// An image passed to the renderer does not have the size its placement expects.
    ImageSizeMismatch {
        index: usize,
        expected: (u32, u32),
        actual: (u32, u32),
    },
    FreeformPositionCount {
        expected: usize,
        actual: usize,
    },
    SplitSizeMismatch {
        index: usize,
        expected: (u32, u32),
        actual: (u32, u32),
    },
    LabelCountMismatch {
        expected: usize,
        actual: usize,
    },
    /// A canvas dimension does not fit in a `u32`.
    SizeOverflow,
    /// Spacing, borders, and labels alone exceed the requested maximum size.
    CannotFit {
        width: u32,
        height: u32,
        max_width: Option<u32>,
        max_height: Option<u32>,
    },
    /// The canvas buffer would exceed the allocation limit.
    CanvasTooLarge {
        width: u32,
        height: u32,
        bytes: u64,
        limit: u64,
    },
}

impl fmt::Display for ComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoImages => write!(f, "at least one image is required"),
            Self::InvalidImageDimensions => write!(f, "image dimensions must be nonzero"),
            Self::InvalidColumns => write!(f, "grid columns must be nonzero"),
            Self::ImageCountMismatch { expected, actual } => {
                write!(f, "expected {expected} images, got {actual}")
            }
            Self::ImageSizeMismatch {
                index,
                expected,
                actual,
            } => write!(
                f,
                "image {index} is {}x{}, but the spec places it at {}x{}",
                actual.0, actual.1, expected.0, expected.1
            ),
            Self::FreeformPositionCount { expected, actual } => {
                write!(f, "expected {expected} freeform positions, got {actual}")
            }
            Self::SplitSizeMismatch {
                index,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "split layout requires all images to have the same dimensions: image {index} is {}x{}, expected {}x{}",
                    actual.0, actual.1, expected.0, expected.1
                )
            }
            Self::LabelCountMismatch { expected, actual } => {
                write!(f, "expected {expected} labels, got {actual}")
            }
            Self::SizeOverflow => write!(
                f,
                "composition dimensions exceed the supported size: a side would be larger than {} px",
                u32::MAX
            ),
            Self::CannotFit {
                width,
                height,
                max_width,
                max_height,
            } => {
                let limit = |bound: &Option<u32>| bound.map_or("any".to_owned(), |b| b.to_string());
                write!(
                    f,
                    "the composition cannot fit in {}x{}: even with images shrunk to one pixel it is {width}x{height}",
                    limit(max_width),
                    limit(max_height)
                )
            }
            Self::CanvasTooLarge {
                width,
                height,
                bytes,
                limit,
            } => write!(
                f,
                "composition dimensions exceed the supported size: a {width}x{height} canvas needs {bytes} bytes, the limit is {limit}"
            ),
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
    let plan = match &options.layout {
        Layout::Grid { columns: 0 } => return Err(ComposeError::InvalidColumns),
        Layout::Grid { columns } => grid(sizes, (*columns).min(sizes.len()), options)?,
        Layout::Stack { direction } => stack(sizes, *direction, options)?,
        Layout::Freeform { positions } => return freeform(sizes, positions),
        Layout::Split { .. } => return split(sizes),
    };
    with_padding(plan, options.spacing)
}

/// Fails on the first image whose size differs from `expected`.
pub(crate) fn require_same_size(
    sizes: impl IntoIterator<Item = (u32, u32)>,
    expected: (u32, u32),
) -> Result<(), ComposeError> {
    match sizes
        .into_iter()
        .enumerate()
        .find(|&(_, size)| size != expected)
    {
        Some((index, actual)) => Err(ComposeError::SplitSizeMismatch {
            index,
            expected,
            actual,
        }),
        None => Ok(()),
    }
}

fn split(sizes: &[(u32, u32)]) -> Result<Plan, ComposeError> {
    let (width, height) = sizes[0];
    require_same_size(sizes.iter().copied(), (width, height))?;
    Ok(Plan {
        positions: vec![(0, 0); sizes.len()],
        width,
        height,
    })
}

/// Returns the index of the image visible at pixel `(x, y)`, or `None` for divider pixels.
///
/// The canvas is cut into `count` equal bands by parallel "/" diagonals that shift
/// 2 px right for every 5 px up and cross mid-height at `k * width / count`.
pub(crate) fn split_region(
    (x, y): (u32, u32),
    (width, height): (u32, u32),
    count: usize,
    divider: u32,
) -> Option<usize> {
    let count = count as i64;
    let band_width = 10 * i64::from(width);
    // 10 * (pixel-centre x shifted back along the diagonal to mid-height), kept integral for exact bands.
    let offset = 10 * i64::from(x) + 4 * i64::from(y) + 7 - 2 * i64::from(height);
    let band = (offset * count).div_euclid(band_width).clamp(0, count - 1);
    let nearest_boundary = ((offset * count) as f64 / band_width as f64)
        .round()
        .clamp(1.0, (count - 1).max(1) as f64);
    let horizontal_distance =
        (offset as f64 - nearest_boundary * band_width as f64 / count as f64).abs() / 10.0;
    let perpendicular_distance = horizontal_distance / (1.0_f64 + 0.4 * 0.4).sqrt();
    let on_divider = count > 1 && perpendicular_distance < f64::from(divider) / 2.0;
    (!on_divider).then_some(band as usize)
}

/// Returns, per band, the first x on the top row that belongs to that band.
pub(crate) fn band_starts(size: (u32, u32), count: usize, divider: u32) -> Vec<u32> {
    (0..count)
        .map(|band| {
            (0..size.0)
                .find(|&x| split_region((x, 0), size, count, divider) == Some(band))
                .unwrap_or(0)
        })
        .collect()
}

/// Returns the fraction of pixel `(x, y)` covered by each visible image, using the same
/// bands as [`split_region`]. Any remaining coverage belongs to the divider.
pub(crate) fn split_coverage(
    (x, y): (u32, u32),
    (width, height): (u32, u32),
    count: usize,
    divider: u32,
) -> impl Iterator<Item = (usize, f64)> + Clone {
    let slant = (1.0_f64 + 0.4 * 0.4).sqrt();
    let position =
        (f64::from(x) + 0.5 + 0.4 * (f64::from(y) + 0.5 - f64::from(height) / 2.0)) / slant;
    let band_width = f64::from(width) / count as f64 / slant;
    let gap = f64::from(divider) / 2.0;
    (0..count)
        .map(move |band| {
            let start = match band {
                0 => f64::NEG_INFINITY,
                _ => band as f64 * band_width + gap,
            };
            let end = match band + 1 == count {
                true => f64::INFINITY,
                false => (band + 1) as f64 * band_width - gap,
            };
            let share = end.min(position + 0.5) - start.max(position - 0.5);
            (band, share.clamp(0.0, 1.0))
        })
        .filter(|&(_, share)| share > 0.0)
}

fn with_padding(plan: Plan, spacing: u32) -> Result<Plan, ComposeError> {
    let margin = spacing.checked_mul(2).ok_or(ComposeError::SizeOverflow)?;
    let width = plan
        .width
        .checked_add(margin)
        .ok_or(ComposeError::SizeOverflow)?;
    let height = plan
        .height
        .checked_add(margin)
        .ok_or(ComposeError::SizeOverflow)?;
    let positions = plan
        .positions
        .into_iter()
        .map(|(x, y)| (x + spacing, y + spacing))
        .collect();
    Ok(Plan {
        positions,
        width,
        height,
    })
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
