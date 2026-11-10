use crate::{layout::ComposeError, CompositionSpec, DynamicImage, ImagePlacement, SpecPosition};
use image::{imageops, imageops::FilterType, RgbaImage};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Target for `--fit-height` / `--fit-width`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitTarget {
    Pixels(u32),
    /// The smallest such dimension among the inputs.
    Min,
}

/// How an image whose aspect ratio differs from its placement is fitted into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementFit {
    /// Scale to fill the placement, cropping the overflow equally from both sides.
    Cover,
    /// Scale to fit inside the placement, leaving transparent padding.
    Contain,
}

/// An exact scale factor `num / den`, never above 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ratio {
    num: u64,
    den: u64,
}

impl Ratio {
    const ONE: Self = Self { num: 1, den: 1 };

    fn shrink_to(target: u32, length: u32) -> Self {
        match target < length {
            true => Self {
                num: u64::from(target),
                den: u64::from(length),
            },
            false => Self::ONE,
        }
    }

    fn min(self, other: Self) -> Self {
        match self.num * other.den <= other.num * self.den {
            true => self,
            false => other,
        }
    }

    fn max(self, other: Self) -> Self {
        match self.min(other) == self {
            true => other,
            false => self,
        }
    }

    fn apply(self, length: u32) -> u32 {
        let scaled = (2 * u64::from(length) * self.num + self.den) / (2 * self.den);
        u32::try_from(scaled.max(1)).unwrap_or(u32::MAX)
    }
}

fn target_length(target: FitTarget, lengths: impl Iterator<Item = u32>) -> u32 {
    match target {
        FitTarget::Pixels(pixels) => pixels,
        FitTarget::Min => lengths.min().unwrap_or(1),
    }
}

/// Shrinks each size, keeping its aspect ratio, so its height is at most `height` and
/// its width at most `width`. Sizes already within the targets are left unchanged.
pub fn fit_sizes(
    sizes: &[(u32, u32)],
    height: Option<FitTarget>,
    width: Option<FitTarget>,
) -> Vec<(u32, u32)> {
    let height = height.map(|target| target_length(target, sizes.iter().map(|size| size.1)));
    let width = width.map(|target| target_length(target, sizes.iter().map(|size| size.0)));
    sizes
        .iter()
        .map(|&(w, h)| {
            let by_height = height.map_or(Ratio::ONE, |target| Ratio::shrink_to(target, h));
            let by_width = width.map_or(Ratio::ONE, |target| Ratio::shrink_to(target, w));
            let ratio = by_height.min(by_width);
            match ratio == by_height {
                true if ratio != Ratio::ONE => (ratio.apply(w), height.unwrap_or(h)),
                _ if ratio != Ratio::ONE => (width.unwrap_or(w), ratio.apply(h)),
                _ => (w, h),
            }
        })
        .collect()
}

/// The largest size every input can be fitted into without enlarging any of them.
pub fn split_target(sizes: &[(u32, u32)]) -> (u32, u32) {
    (
        sizes.iter().map(|size| size.0).min().unwrap_or(1),
        sizes.iter().map(|size| size.1).min().unwrap_or(1),
    )
}

/// Scales a length by `scale` in `(0, 1]`, rounding down but never below one pixel.
pub fn scale_length(length: u32, scale: f64) -> u32 {
    ((f64::from(length) * scale).floor() as u32).clamp(1, length.max(1))
}

fn fits(spec: &CompositionSpec, bounds: (Option<u32>, Option<u32>)) -> bool {
    bounds.0.is_none_or(|width| spec.width <= width)
        && bounds.1.is_none_or(|height| spec.height <= height)
}

/// Finds the largest image scale in `(0, 1]` whose composition fits within `bounds`.
///
/// `compose` builds the composition with every image (and freeform position) scaled by the
/// given factor. Spacing, borders, and labels keep their size, so only images shrink.
pub fn fit_within(
    bounds: (Option<u32>, Option<u32>),
    compose: impl Fn(f64) -> Result<CompositionSpec, ComposeError>,
) -> Result<(CompositionSpec, f64), ComposeError> {
    let full = compose(1.0)?;
    if fits(&full, bounds) {
        return Ok((full, 1.0));
    }
    let smallest = compose(0.0)?;
    if !fits(&smallest, bounds) {
        return Err(ComposeError::CannotFit {
            width: smallest.width,
            height: smallest.height,
            max_width: bounds.0,
            max_height: bounds.1,
        });
    }
    let (mut low, mut high, mut best) = (0.0_f64, 1.0_f64, smallest);
    for _ in 0..48 {
        let middle = (low + high) / 2.0;
        let candidate = compose(middle)?;
        if fits(&candidate, bounds) {
            low = middle;
            best = candidate;
        } else {
            high = middle;
        }
    }
    Ok((best, low))
}

fn centered(outer: u32, inner: u32) -> u32 {
    outer.saturating_sub(inner) / 2
}

fn cover(image: &DynamicImage, (width, height): (u32, u32)) -> DynamicImage {
    let ratio =
        Ratio::shrink_to(width, image.width()).max(Ratio::shrink_to(height, image.height()));
    let scaled = (
        ratio.apply(image.width()).max(width),
        ratio.apply(image.height()).max(height),
    );
    let resized = image.resize_exact(scaled.0, scaled.1, FilterType::Lanczos3);
    resized.crop_imm(
        centered(scaled.0, width),
        centered(scaled.1, height),
        width,
        height,
    )
}

fn contain(image: &DynamicImage, (width, height): (u32, u32)) -> DynamicImage {
    let ratio =
        Ratio::shrink_to(width, image.width()).min(Ratio::shrink_to(height, image.height()));
    let scaled = (
        ratio.apply(image.width()).min(width),
        ratio.apply(image.height()).min(height),
    );
    let resized = image.resize_exact(scaled.0, scaled.1, FilterType::Lanczos3);
    let mut canvas = RgbaImage::new(width, height);
    imageops::overlay(
        &mut canvas,
        &resized,
        i64::from(centered(width, scaled.0)),
        i64::from(centered(height, scaled.1)),
    );
    DynamicImage::ImageRgba8(canvas)
}

/// Checks that `source` can be shrunk (never enlarged) to the placement's size.
pub fn check_source_size(
    index: usize,
    placement: &ImagePlacement,
    source: (u32, u32),
) -> Result<(), ComposeError> {
    let expected = (placement.width, placement.height);
    match source.0 >= expected.0 && source.1 >= expected.1 {
        true => Ok(()),
        false => Err(ComposeError::ImageSizeMismatch {
            index,
            expected,
            actual: source,
        }),
    }
}

/// Produces the image at the size its placement expects, shrinking it if needed.
pub fn prepare_image<'a>(
    index: usize,
    image: &'a DynamicImage,
    placement: &ImagePlacement,
) -> Result<Cow<'a, DynamicImage>, ComposeError> {
    let size = (placement.width, placement.height);
    if (image.width(), image.height()) == size {
        return Ok(Cow::Borrowed(image));
    }
    check_source_size(index, placement, (image.width(), image.height()))?;
    Ok(Cow::Owned(match placement.fit {
        Some(PlacementFit::Cover) => cover(image, size),
        Some(PlacementFit::Contain) => contain(image, size),
        None => image.resize_exact(size.0, size.1, FilterType::Lanczos3),
    }))
}

/// Scales freeform positions with the images they place.
pub fn scale_positions(positions: &[SpecPosition], scale: f64) -> Vec<SpecPosition> {
    positions
        .iter()
        .map(|position| SpecPosition {
            x: (f64::from(position.x) * scale).floor() as u32,
            y: (f64::from(position.y) * scale).floor() as u32,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        compose_spec, Alignment, CompositionParams, ImageMetadata, RgbColor, SpecFrame, SpecLayout,
        StackDirection,
    };
    use image::{GenericImageView, Rgba};

    #[test]
    fn fit_height_shrinks_taller_images_to_the_target_keeping_aspect() {
        assert_eq!(
            fit_sizes(
                &[(1600, 800), (300, 600)],
                Some(FitTarget::Pixels(400)),
                None
            ),
            [(800, 400), (200, 400)]
        );
    }

    #[test]
    fn fit_height_leaves_shorter_images_unchanged() {
        assert_eq!(
            fit_sizes(&[(100, 50), (800, 400)], Some(FitTarget::Pixels(200)), None),
            [(100, 50), (400, 200)]
        );
    }

    #[test]
    fn fit_height_min_matches_the_shortest_image() {
        assert_eq!(
            fit_sizes(
                &[(1280, 800), (1024, 768), (600, 300)],
                Some(FitTarget::Min),
                None
            ),
            [(480, 300), (400, 300), (600, 300)]
        );
    }

    #[test]
    fn fit_width_min_matches_the_narrowest_image() {
        assert_eq!(
            fit_sizes(&[(800, 400), (400, 300)], None, Some(FitTarget::Min)),
            [(400, 200), (400, 300)]
        );
    }

    #[test]
    fn both_targets_apply_the_tighter_one() {
        assert_eq!(
            fit_sizes(
                &[(1000, 100), (100, 1000)],
                Some(FitTarget::Pixels(500)),
                Some(FitTarget::Pixels(500))
            ),
            [(500, 50), (50, 500)]
        );
    }

    #[test]
    fn no_targets_keep_every_size() {
        assert_eq!(fit_sizes(&[(3, 4), (5, 6)], None, None), [(3, 4), (5, 6)]);
    }

    #[test]
    fn fitted_lengths_round_to_the_nearest_pixel_and_stay_positive() {
        assert_eq!(
            fit_sizes(&[(3, 2)], Some(FitTarget::Pixels(1)), None),
            [(2, 1)]
        );
        assert_eq!(
            fit_sizes(&[(1, 1000)], Some(FitTarget::Pixels(10)), None),
            [(1, 10)]
        );
    }

    #[test]
    fn fitting_never_enlarges() {
        let sizes = [(7, 13), (400, 3), (1, 1), (999, 998)];
        for target in [FitTarget::Pixels(1), FitTarget::Pixels(500), FitTarget::Min] {
            for (fitted, original) in fit_sizes(&sizes, Some(target), Some(target))
                .into_iter()
                .zip(sizes)
            {
                assert!(
                    fitted.0 <= original.0 && fitted.1 <= original.1,
                    "{target:?}"
                );
            }
        }
    }

    #[test]
    fn split_target_takes_the_smallest_width_and_height_separately() {
        assert_eq!(split_target(&[(1280, 800), (1024, 900)]), (1024, 800));
    }

    #[test]
    fn scale_length_rounds_down_and_never_reaches_zero() {
        assert_eq!(scale_length(100, 0.5), 50);
        assert_eq!(scale_length(99, 0.5), 49);
        assert_eq!(scale_length(10, 0.0), 1);
        assert_eq!(scale_length(10, 1.0), 10);
    }

    #[test]
    fn scale_positions_rounds_down() {
        assert_eq!(
            scale_positions(&[SpecPosition { x: 101, y: 3 }], 0.5),
            [SpecPosition { x: 50, y: 1 }]
        );
    }

    fn compose_scaled(
        sizes: &[(u32, u32)],
        spacing: u32,
        frame: Option<SpecFrame>,
    ) -> impl Fn(f64) -> Result<CompositionSpec, ComposeError> + '_ {
        move |scale| {
            let images = sizes
                .iter()
                .map(|&(width, height)| ImageMetadata {
                    name: "image".into(),
                    width: scale_length(width, scale),
                    height: scale_length(height, scale),
                })
                .collect::<Vec<_>>();
            compose_spec(
                &images,
                &CompositionParams {
                    layout: SpecLayout::Stack {
                        direction: StackDirection::Horizontal,
                        spacing,
                        alignment: Alignment::Start,
                    },
                    background_color: RgbColor::default(),
                    background_gradient: None,
                    frame: frame.clone(),
                },
            )
        }
    }

    #[test]
    fn composition_already_within_bounds_is_unchanged() {
        let sizes = [(100, 50), (100, 50)];
        let (spec, scale) =
            fit_within((Some(200), Some(50)), compose_scaled(&sizes, 0, None)).unwrap();

        assert_eq!(scale, 1.0);
        assert_eq!((spec.width, spec.height), (200, 50));
    }

    #[test]
    fn only_images_shrink_while_spacing_keeps_its_size() {
        let sizes = [(1000, 500), (1000, 500)];
        let (spec, _) = fit_within((Some(520), None), compose_scaled(&sizes, 10, None)).unwrap();

        assert!(spec.width <= 520);
        assert!(spec.width >= 515, "{}", spec.width);
        let gap = spec.images[1].position.x - (spec.images[0].position.x + spec.images[0].width);
        assert_eq!(gap, 10);
        assert_eq!(spec.images[0].position.x, 10);
    }

    #[test]
    fn height_bound_alone_is_respected() {
        let sizes = [(300, 900)];
        let (spec, scale) = fit_within((None, Some(300)), compose_scaled(&sizes, 0, None)).unwrap();

        assert_eq!(spec.height, 300);
        assert_eq!(spec.width, 100);
        assert!((1.0 / 3.0..301.0 / 900.0).contains(&scale), "{scale}");
    }

    #[test]
    fn both_bounds_are_respected() {
        let sizes = [(800, 800), (800, 200)];
        let (spec, _) =
            fit_within((Some(500), Some(100)), compose_scaled(&sizes, 4, None)).unwrap();

        assert!(
            spec.width <= 500 && spec.height <= 100,
            "{}x{}",
            spec.width,
            spec.height
        );
    }

    #[test]
    fn borders_keep_their_width_when_images_shrink() {
        let frame = SpecFrame {
            labels: vec![String::new()],
            border_width: 5,
            border_color: RgbColor::default(),
            label_size: 20,
        };
        let sizes = [(1000, 1000)];
        let (spec, _) =
            fit_within((Some(110), None), compose_scaled(&sizes, 0, Some(frame))).unwrap();

        assert_eq!(spec.width, 110);
        assert_eq!(spec.images[0].width, 100);
        assert_eq!(spec.images[0].position.x, 5);
    }

    #[test]
    fn bounds_smaller_than_the_fixed_overhead_cannot_fit() {
        let sizes = [(100, 100), (100, 100)];

        assert_eq!(
            fit_within((Some(20), None), compose_scaled(&sizes, 10, None)).err(),
            Some(ComposeError::CannotFit {
                width: 32,
                height: 21,
                max_width: Some(20),
                max_height: None,
            })
        );
    }

    #[test]
    fn fitted_composition_is_the_largest_that_fits() {
        let sizes = [(997, 389), (613, 777)];
        let compose = compose_scaled(&sizes, 3, None);
        let (spec, scale) = fit_within((Some(640), Some(480)), &compose).unwrap();
        let larger = compose((scale * 1.01).min(1.0)).unwrap();

        assert!(spec.width <= 640 && spec.height <= 480);
        assert!(larger.width > 640 || larger.height > 480);
    }

    fn placement(width: u32, height: u32, fit: Option<PlacementFit>) -> ImagePlacement {
        ImagePlacement {
            name: "image".into(),
            path: None,
            width,
            height,
            fit,
            position: SpecPosition { x: 0, y: 0 },
        }
    }

    fn halves(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_fn(width, height, |x, _| {
            if x < width / 2 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 255, 255])
            }
        }))
    }

    #[test]
    fn prepare_borrows_an_image_that_already_matches() {
        let image = halves(4, 2);

        assert!(matches!(
            prepare_image(0, &image, &placement(4, 2, None)).unwrap(),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn prepare_scales_down_to_the_placement() {
        let image = halves(40, 20);
        let prepared = prepare_image(0, &image, &placement(20, 10, None)).unwrap();

        assert_eq!(prepared.dimensions(), (20, 10));
        assert_eq!(prepared.get_pixel(2, 5).0, [255, 0, 0, 255]);
        assert_eq!(prepared.get_pixel(17, 5).0, [0, 0, 255, 255]);
    }

    #[test]
    fn prepare_refuses_to_enlarge() {
        let image = halves(4, 4);

        assert_eq!(
            prepare_image(3, &image, &placement(8, 4, None)).err(),
            Some(ComposeError::ImageSizeMismatch {
                index: 3,
                expected: (8, 4),
                actual: (4, 4)
            })
        );
    }

    #[test]
    fn cover_fills_the_placement_and_crops_the_sides() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_fn(30, 10, |x, _| match x {
            0..10 => Rgba([255, 0, 0, 255]),
            10..20 => Rgba([0, 255, 0, 255]),
            _ => Rgba([0, 0, 255, 255]),
        }));
        let prepared =
            prepare_image(0, &image, &placement(10, 10, Some(PlacementFit::Cover))).unwrap();

        assert_eq!(prepared.dimensions(), (10, 10));
        assert!((0..10).all(|x| prepared.get_pixel(x, 5).0 == [0, 255, 0, 255]));
    }

    #[test]
    fn cover_scales_before_cropping() {
        let image = halves(40, 20);
        let prepared =
            prepare_image(0, &image, &placement(20, 8, Some(PlacementFit::Cover))).unwrap();

        assert_eq!(prepared.dimensions(), (20, 8));
        assert_eq!(prepared.get_pixel(1, 4).0, [255, 0, 0, 255]);
        assert_eq!(prepared.get_pixel(18, 4).0, [0, 0, 255, 255]);
    }

    #[test]
    fn contain_letterboxes_with_transparent_padding() {
        let image = halves(40, 10);
        let prepared =
            prepare_image(0, &image, &placement(20, 10, Some(PlacementFit::Contain))).unwrap();

        assert_eq!(prepared.dimensions(), (20, 10));
        assert_eq!(prepared.get_pixel(10, 0).0[3], 0);
        assert_eq!(prepared.get_pixel(10, 9).0[3], 0);
        assert_eq!(prepared.get_pixel(1, 5).0, [255, 0, 0, 255]);
        assert_eq!(prepared.get_pixel(18, 5).0, [0, 0, 255, 255]);
    }

    #[test]
    fn source_size_check_accepts_equal_or_larger_sources() {
        let target = placement(10, 10, None);

        assert!(check_source_size(0, &target, (10, 10)).is_ok());
        assert!(check_source_size(0, &target, (11, 10)).is_ok());
        assert!(check_source_size(0, &target, (9, 20)).is_err());
        assert!(check_source_size(0, &target, (20, 9)).is_err());
    }

    #[test]
    fn placement_fit_serializes_in_snake_case_and_is_omitted_when_absent() {
        let covered = placement(1, 1, Some(PlacementFit::Cover));

        assert_eq!(serde_json::to_value(&covered).unwrap()["fit"], "cover");
        assert!(serde_json::to_value(placement(1, 1, None))
            .unwrap()
            .get("fit")
            .is_none());
    }
}
