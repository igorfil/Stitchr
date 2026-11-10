use crate::{frame, layout, CompositionSpec, SpecLayout};

/// A composition that renders, but probably not as intended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelWarning {
    /// The label tab is wider than the image or split band it belongs to.
    Overflow {
        index: usize,
        tab_width: u32,
        available_width: u32,
    },
    /// The bundled font has no glyph for these characters, so they render as gaps.
    MissingGlyphs { index: usize, characters: Vec<char> },
}

fn available_widths(spec: &CompositionSpec) -> Vec<(u32, u32)> {
    let Some(frame) = &spec.frame else {
        return Vec::new();
    };
    let (left, top) = frame::inset(frame);
    match spec.layout {
        SpecLayout::Split { divider } => {
            let size = (
                spec.width.saturating_sub(left.saturating_mul(2)),
                spec.height.saturating_sub(top.saturating_add(left)),
            );
            let starts = layout::band_starts(size, frame.labels.len(), divider);
            let ends = starts
                .iter()
                .skip(1)
                .map(|&start| left + start)
                .chain(std::iter::once(spec.width));
            starts
                .iter()
                .zip(ends)
                .enumerate()
                .map(|(index, (&start, end))| match index {
                    0 => (0, end),
                    _ => (left + start, end),
                })
                .collect()
        }
        _ => spec
            .images
            .iter()
            .map(|image| (0, image.width.saturating_add(left.saturating_mul(2))))
            .collect(),
    }
}

fn overflow(
    spec: &CompositionSpec,
    index: usize,
    label: &str,
    (start, end): (u32, u32),
) -> Option<LabelWarning> {
    let frame = spec.frame.as_ref()?;
    let left_inset = match (&spec.layout, index) {
        (SpecLayout::Split { .. }, 1..) => 0,
        _ => frame.border_width,
    };
    let tab_width = frame::tab_top_width(frame, label, left_inset);
    let available_width = end.saturating_sub(start);
    (!label.is_empty() && tab_width > available_width).then_some(LabelWarning::Overflow {
        index,
        tab_width,
        available_width,
    })
}

/// Reports labels that overflow their image or band and characters the font cannot draw.
pub fn label_warnings(spec: &CompositionSpec) -> Vec<LabelWarning> {
    let Some(frame) = &spec.frame else {
        return Vec::new();
    };
    let widths = available_widths(spec);
    frame
        .labels
        .iter()
        .zip(widths)
        .enumerate()
        .flat_map(|(index, (label, extent))| {
            let characters = frame::missing_glyphs(label);
            let missing = (!characters.is_empty())
                .then_some(LabelWarning::MissingGlyphs { index, characters });
            overflow(spec, index, label, extent)
                .into_iter()
                .chain(missing)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        compose_spec, Alignment, CompositionParams, ImageMetadata, RgbColor, SpecFrame,
        StackDirection,
    };

    fn metadata(sizes: &[(u32, u32)]) -> Vec<ImageMetadata> {
        sizes
            .iter()
            .enumerate()
            .map(|(index, &(width, height))| ImageMetadata {
                name: format!("image-{index}.png"),
                width,
                height,
            })
            .collect()
    }

    fn framed(labels: &[&str]) -> SpecFrame {
        SpecFrame {
            labels: labels.iter().map(|label| (*label).to_owned()).collect(),
            border_width: 2,
            border_color: RgbColor::default(),
            label_size: 20,
        }
    }

    fn warnings(sizes: &[(u32, u32)], layout: SpecLayout, labels: &[&str]) -> Vec<LabelWarning> {
        let images = metadata(sizes);
        let spec = compose_spec(
            &images,
            &CompositionParams {
                layout,
                background_color: RgbColor::default(),
                background_gradient: None,
                frame: Some(framed(labels)),
            },
        )
        .unwrap();
        label_warnings(&spec)
    }

    fn horizontal() -> SpecLayout {
        SpecLayout::Stack {
            direction: StackDirection::Horizontal,
            spacing: 0,
            alignment: Alignment::Start,
        }
    }

    #[test]
    fn composition_without_labels_has_no_warnings() {
        let images = metadata(&[(10, 10)]);
        let spec = compose_spec(
            &images,
            &CompositionParams {
                layout: horizontal(),
                background_color: RgbColor::default(),
                background_gradient: None,
                frame: None,
            },
        )
        .unwrap();

        assert_eq!(label_warnings(&spec), []);
    }

    #[test]
    fn short_label_on_a_wide_image_has_no_warnings() {
        assert_eq!(warnings(&[(400, 100)], horizontal(), &["Before"]), []);
    }

    #[test]
    fn empty_label_never_overflows() {
        assert_eq!(warnings(&[(1, 1)], horizontal(), &[""]), []);
    }

    #[test]
    fn long_label_on_a_narrow_image_overflows() {
        let result = warnings(
            &[(400, 100), (20, 100)],
            horizontal(),
            &["A", "Much too long"],
        );

        assert_eq!(result.len(), 1);
        let LabelWarning::Overflow {
            index,
            tab_width,
            available_width,
        } = result[0]
        else {
            panic!("expected overflow, got {result:?}");
        };
        assert_eq!(index, 1);
        assert_eq!(available_width, 24);
        assert!(tab_width > available_width);
    }

    #[test]
    fn overflow_threshold_is_the_framed_tile_width() {
        let frame = framed(&["Label"]);
        let tab = frame::tab_top_width(&frame, "Label", frame.border_width);
        let fits = tab - 2 * frame.border_width;

        assert_eq!(warnings(&[(fits, 50)], horizontal(), &["Label"]), []);
        assert_eq!(
            warnings(&[(fits - 1, 50)], horizontal(), &["Label"]),
            [LabelWarning::Overflow {
                index: 0,
                tab_width: tab,
                available_width: tab - 1,
            }]
        );
    }

    #[test]
    fn characters_missing_from_the_font_are_reported_once_in_order() {
        let result = warnings(&[(800, 100)], horizontal(), &["日本日a本"]);

        assert_eq!(
            result,
            [LabelWarning::MissingGlyphs {
                index: 0,
                characters: vec!['日', '本'],
            }]
        );
    }

    #[test]
    fn latin_accents_and_punctuation_are_supported() {
        assert_eq!(
            warnings(
                &[(2000, 100)],
                horizontal(),
                &["Café — Ünïcödé, 100% (ok)!"]
            ),
            []
        );
    }

    #[test]
    fn one_label_can_both_overflow_and_miss_glyphs() {
        let result = warnings(&[(4, 4)], horizontal(), &["日本語のラベル"]);

        assert!(matches!(result[0], LabelWarning::Overflow { index: 0, .. }));
        assert!(matches!(
            result[1],
            LabelWarning::MissingGlyphs { index: 0, .. }
        ));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn split_labels_are_measured_against_their_band() {
        let wide = warnings(
            &[(800, 100), (800, 100)],
            SpecLayout::Split { divider: 0 },
            &["Light", "Dark"],
        );
        let narrow = warnings(
            &[(60, 100), (60, 100)],
            SpecLayout::Split { divider: 0 },
            &["Light theme", "Dark theme"],
        );

        assert_eq!(wide, []);
        assert_eq!(
            narrow
                .iter()
                .map(|warning| match warning {
                    LabelWarning::Overflow { index, .. } => *index,
                    LabelWarning::MissingGlyphs { .. } => usize::MAX,
                })
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }

    #[test]
    fn overflow_is_measured_against_the_drawn_size() {
        let images = metadata(&[(400, 100)]);
        let mut spec = compose_spec(
            &images,
            &CompositionParams {
                layout: horizontal(),
                background_color: RgbColor::default(),
                background_gradient: None,
                frame: Some(framed(&["Before"])),
            },
        )
        .unwrap();
        spec.images[0].width = 10;

        assert!(matches!(
            label_warnings(&spec)[..],
            [LabelWarning::Overflow {
                available_width: 14,
                ..
            }]
        ));
    }

    #[test]
    fn split_band_widths_sum_to_the_canvas_width() {
        let images = metadata(&[(90, 30), (90, 30), (90, 30)]);
        let spec = compose_spec(
            &images,
            &CompositionParams {
                layout: SpecLayout::Split { divider: 0 },
                background_color: RgbColor::default(),
                background_gradient: None,
                frame: Some(framed(&["a", "b", "c"])),
            },
        )
        .unwrap();

        let widths = available_widths(&spec);

        assert_eq!(widths.first().unwrap().0, 0);
        assert_eq!(widths.last().unwrap().1, spec.width);
        assert!(widths.windows(2).all(|pair| pair[0].1 == pair[1].0));
    }
}
