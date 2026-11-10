use crate::{Cli, CliLayout};
use clap::{CommandFactory, ValueEnum};
use serde_json::{json, Value};

/// When an option has an effect; outside these conditions an explicit value is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Applies {
    Always,
    Layouts(&'static [CliLayout]),
    Labels,
    Gradient,
    LinearGradient,
    JpegOutput,
    Render,
    FileOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OptionRule {
    pub id: &'static str,
    pub applies: Applies,
    pub min: Option<u64>,
    pub max: Option<u64>,
}

const NOT_SPLIT: &[CliLayout] = &[
    CliLayout::Grid,
    CliLayout::Horizontal,
    CliLayout::Vertical,
    CliLayout::Freeform,
];
const STACKS_AND_GRID: &[CliLayout] =
    &[CliLayout::Grid, CliLayout::Horizontal, CliLayout::Vertical];
const U32_MAX: u64 = u32::MAX as u64;

const fn rule(id: &'static str, applies: Applies) -> OptionRule {
    OptionRule {
        id,
        applies,
        min: None,
        max: None,
    }
}

const fn ranged(id: &'static str, applies: Applies, min: u64, max: u64) -> OptionRule {
    OptionRule {
        id,
        applies,
        min: Some(min),
        max: Some(max),
    }
}

pub(crate) const RULES: &[OptionRule] = &[
    ranged("columns", Applies::Layouts(&[CliLayout::Grid]), 1, U32_MAX),
    ranged("spacing", Applies::Layouts(STACKS_AND_GRID), 0, U32_MAX),
    rule("alignment", Applies::Layouts(STACKS_AND_GRID)),
    ranged(
        "position",
        Applies::Layouts(&[CliLayout::Freeform]),
        0,
        U32_MAX,
    ),
    ranged("divider", Applies::Layouts(&[CliLayout::Split]), 0, U32_MAX),
    ranged("border_width", Applies::Labels, 0, U32_MAX),
    rule("border_color", Applies::Labels),
    ranged("label_size", Applies::Labels, 1, U32_MAX),
    ranged("fit_height", Applies::Layouts(NOT_SPLIT), 1, U32_MAX),
    ranged("fit_width", Applies::Layouts(NOT_SPLIT), 1, U32_MAX),
    rule("fit", Applies::Layouts(&[CliLayout::Split])),
    ranged("max_width", Applies::Always, 1, U32_MAX),
    ranged("max_height", Applies::Always, 1, U32_MAX),
    rule("gradient_kind", Applies::Gradient),
    ranged("gradient_angle", Applies::LinearGradient, 0, 359),
    ranged("jpeg_quality", Applies::JpegOutput, 1, 100),
    rule("output", Applies::Render),
    rule("format", Applies::Render),
    rule("force", Applies::FileOutput),
    rule("no_clobber", Applies::FileOutput),
    rule("mkdir", Applies::FileOutput),
];

pub(crate) fn find(id: &str) -> Option<&'static OptionRule> {
    RULES.iter().find(|rule| rule.id == id)
}

pub(crate) fn find_by_long(long: &str) -> Option<&'static OptionRule> {
    let id = Cli::command()
        .get_arguments()
        .find(|arg| {
            arg.get_long()
                .is_some_and(|name| format!("--{name}") == long)
        })
        .map(|arg| arg.get_id().to_string())?;
    find(&id)
}

pub(crate) fn value_name(value: impl ValueEnum) -> String {
    value
        .to_possible_value()
        .map(|value| value.get_name().to_owned())
        .unwrap_or_default()
}

impl OptionRule {
    /// Rust-style range text, e.g. `1..` or `1..=100`.
    pub(crate) fn range(&self) -> Option<String> {
        match (self.min, self.max) {
            (Some(min), Some(U32_MAX)) => Some(format!("{min}..")),
            (Some(min), Some(max)) => Some(format!("{min}..={max}")),
            _ => None,
        }
    }
}

/// Schema fields describing when an option has an effect; `None` means always.
pub(crate) fn applicability(rule: Option<&OptionRule>) -> Value {
    let layouts = |layouts: &[CliLayout]| {
        layouts
            .iter()
            .map(|layout| value_name(*layout))
            .collect::<Vec<_>>()
    };
    let mut fields = json!({
        "applies_to_layouts": Value::Null,
        "requires": [],
        "ignored_with": []
    });
    let Some(rule) = rule else {
        return fields;
    };
    match rule.applies {
        Applies::Always => {}
        Applies::Layouts(allowed) => fields["applies_to_layouts"] = json!(layouts(allowed)),
        Applies::Labels => fields["requires"] = json!(["--label"]),
        Applies::Gradient => fields["requires"] = json!(["--background-gradient"]),
        Applies::LinearGradient => {
            fields["requires"] = json!(["--background-gradient"]);
            fields["applies_to_gradient_kinds"] = json!(["linear"]);
        }
        Applies::JpegOutput => {
            fields["applies_to_formats"] = json!(["jpeg"]);
            fields["ignored_with"] = json!(["--print-spec"]);
        }
        Applies::Render => fields["ignored_with"] = json!(["--print-spec"]),
        Applies::FileOutput => fields["ignored_with"] = json!(["--print-spec", "-o -"]),
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn every_rule_names_a_real_argument() {
        let command = Cli::command();
        for rule in RULES {
            assert!(
                command.get_arguments().any(|arg| arg.get_id() == rule.id),
                "{}",
                rule.id
            );
        }
    }

    #[test]
    fn rule_ids_are_unique() {
        for (index, rule) in RULES.iter().enumerate() {
            assert!(
                RULES[index + 1..].iter().all(|other| other.id != rule.id),
                "{}",
                rule.id
            );
        }
    }

    #[test]
    fn ranges_agree_with_the_parser() {
        for rule in RULES.iter().filter(|rule| rule.min.is_some()) {
            let long = format!("--{}", rule.id.replace('_', "-"));
            let parse = |value: u64| {
                let value = value.to_string();
                let mut args = vec!["stitchr", "a.png", long.as_str(), value.as_str()];
                if rule.id.starts_with("fit_") && value == "0" {
                    return false;
                }
                if rule.id == "position" {
                    args.push("0");
                }
                crate::Cli::try_parse_from(args).is_ok()
            };
            let (min, max) = (rule.min.unwrap(), rule.max.unwrap());

            assert!(parse(min), "{long} {min}");
            assert!(parse(max), "{long} {max}");
            assert!(min == 0 || !parse(min - 1), "{long} below {min}");
            assert!(!parse(max + 1), "{long} above {max}");
        }
    }

    #[test]
    fn range_text_uses_open_ended_form_for_u32_limits() {
        assert_eq!(find("columns").unwrap().range().as_deref(), Some("1.."));
        assert_eq!(find("spacing").unwrap().range().as_deref(), Some("0.."));
        assert_eq!(
            find("jpeg_quality").unwrap().range().as_deref(),
            Some("1..=100")
        );
        assert_eq!(
            find("gradient_angle").unwrap().range().as_deref(),
            Some("0..=359")
        );
        assert_eq!(find("alignment").unwrap().range(), None);
    }

    #[test]
    fn rules_are_found_by_long_flag() {
        assert_eq!(find_by_long("--border-width").unwrap().id, "border_width");
        assert_eq!(find_by_long("--jpeg-quality").unwrap().id, "jpeg_quality");
        assert!(find_by_long("--json").is_none());
        assert!(find_by_long("--nope").is_none());
    }

    #[test]
    fn applicability_describes_layout_restrictions() {
        assert_eq!(
            applicability(find("spacing")),
            json!({
                "applies_to_layouts": ["grid", "horizontal", "vertical"],
                "requires": [],
                "ignored_with": []
            })
        );
    }

    #[test]
    fn applicability_describes_requirements_and_modes() {
        assert_eq!(
            applicability(find("gradient_angle")),
            json!({
                "applies_to_layouts": null,
                "requires": ["--background-gradient"],
                "applies_to_gradient_kinds": ["linear"],
                "ignored_with": []
            })
        );
        assert_eq!(
            applicability(find("jpeg_quality")),
            json!({
                "applies_to_layouts": null,
                "requires": [],
                "applies_to_formats": ["jpeg"],
                "ignored_with": ["--print-spec"]
            })
        );
        assert_eq!(
            applicability(find("mkdir"))["ignored_with"],
            json!(["--print-spec", "-o -"])
        );
    }

    #[test]
    fn unrestricted_options_report_no_restrictions() {
        assert_eq!(
            applicability(None),
            json!({ "applies_to_layouts": null, "requires": [], "ignored_with": [] })
        );
    }
}
