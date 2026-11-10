use crate::{error, options, warnings, Cli, DEFAULT_SCHEMA_FORMAT, SCHEMA_VERSION};
use clap::{Arg, ArgAction, Command, CommandFactory, ValueHint};
use serde_json::{json, Map, Value};

/// Arguments that control the CLI itself rather than describe a composition request.
const META_ARGUMENTS: [&str; 4] = ["help", "version", "help_json", "json"];

fn argument_schema(command: &Command, arg: &Arg) -> Value {
    let choices: Vec<_> = arg
        .get_possible_values()
        .into_iter()
        .filter(|value| !value.is_hide_set())
        .map(|value| value.get_name().to_owned())
        .collect();
    let boolean = matches!(
        arg.get_action(),
        ArgAction::SetTrue | ArgAction::Help | ArgAction::Version
    );
    let path = arg.get_value_hint() == ValueHint::AnyPath;
    let color = matches!(
        arg.get_id().as_str(),
        "background_color" | "border_color" | "background_gradient"
    );
    let text = arg.get_id().as_str() == "label";
    let value_type = if boolean {
        "boolean"
    } else if path || color || text || !choices.is_empty() {
        "string"
    } else {
        "integer"
    };
    let default = if boolean {
        json!(false)
    } else {
        match arg.get_default_values().first() {
            Some(value) if value_type == "integer" => value
                .to_string_lossy()
                .parse::<u64>()
                .map_or(Value::Null, |number| json!(number)),
            Some(value) => json!(value.to_string_lossy()),
            None => Value::Null,
        }
    };
    let arity = arg.get_num_args();
    let rule = options::find(arg.get_id().as_str());
    let names_conflict = |first: &Arg, second: &Arg| {
        command
            .get_arg_conflicts_with(first)
            .iter()
            .any(|conflict| conflict.get_id() == second.get_id())
    };
    let conflicts_with = command
        .get_arguments()
        .filter(|other| names_conflict(arg, other) || names_conflict(other, arg))
        .filter_map(|other| other.get_long().map(|long| format!("--{long}")))
        .collect::<Vec<_>>();

    let mut schema = json!({
        "name": arg.get_id().as_str(),
        "long": arg.get_long().map(|long| format!("--{long}")),
        "short": arg.get_short().map(|short| format!("-{short}")),
        "aliases": arg.get_visible_aliases().unwrap_or_default().into_iter().map(|alias| format!("--{alias}")).collect::<Vec<_>>(),
        "kind": if arg.is_positional() { "positional" } else { "option" },
        "type": value_type,
        "format": if path { Some("path") } else if color { Some("color") } else { None },
        "enum": choices,
        "default": default,
        "default_missing_value": (arg.get_id() == "help_json").then_some(DEFAULT_SCHEMA_FORMAT),
        "require_equals": arg.is_require_equals_set(),
        "minimum": rule.and_then(|rule| rule.min),
        "maximum": rule.and_then(|rule| rule.max),
        "required": arg.is_required_set() || arg.is_positional(),
        "required_unless": if arg.is_positional() { vec!["--help-json", "--schema"] } else { vec![] },
        "conflicts_with": conflicts_with,
        "repeatable": matches!(arg.get_action(), ArgAction::Append),
        "values_per_occurrence": {
            "min": arity.map(|range| range.min_values()).unwrap_or(0),
            "max": arity.and_then(|range| (range.max_values() != usize::MAX).then_some(range.max_values()))
        },
        "value_names": if boolean { None } else { arg.get_value_names().map(|names| names.iter().map(ToString::to_string).collect::<Vec<_>>()) },
        "help": arg.get_help().map(ToString::to_string)
    });
    if let (Some(schema), Value::Object(fields)) =
        (schema.as_object_mut(), options::applicability(rule))
    {
        schema.extend(fields);
    }
    schema
}

fn built_command() -> Command {
    let mut command = Cli::command();
    command.build();
    command
}

fn exit_codes() -> Value {
    json!([
        { "code": 0, "meaning": "success, including help, version, schema, and spec inspection" },
        { "code": 1, "meaning": "input, composition, rendering, output-format, serialization, or file-write failure" },
        { "code": 2, "meaning": "argument parsing or validation failure" }
    ])
}

pub(crate) fn schema() -> Value {
    let command = built_command();
    let usage = command.clone().render_usage().to_string();
    let after_help = command
        .get_after_help()
        .map(|text| text.to_string())
        .unwrap_or_default();
    let examples = after_help
        .lines()
        .skip(1)
        .map(str::trim)
        .collect::<Vec<_>>();
    let arguments = command
        .get_arguments()
        .map(|arg| argument_schema(&command, arg))
        .collect::<Vec<_>>();
    let error_codes = error::ERROR_CODES
        .iter()
        .map(|&(code, description)| {
            json!({
                "code": code,
                "exit_code": error::exit_code_for(code),
                "description": description
            })
        })
        .collect::<Vec<_>>();
    let warning_codes = warnings::WARNING_CODES
        .iter()
        .map(|&(code, description)| json!({ "code": code, "description": description }))
        .collect::<Vec<_>>();

    json!({
        "schema_version": SCHEMA_VERSION,
        "name": command.get_name(),
        "version": command.get_version(),
        "description": command.get_long_about().map(ToString::to_string),
        "usage": usage,
        "examples": examples,
        "arguments": arguments,
        "error_codes": error_codes,
        "warning_codes": warning_codes,
        "exit_codes": exit_codes()
    })
}

fn scalar_json_schema(argument: &Value) -> Value {
    let mut scalar = Map::new();
    scalar.insert("type".into(), argument["type"].clone());
    if let Some(choices) = argument["enum"]
        .as_array()
        .filter(|choices| !choices.is_empty())
    {
        scalar.insert("enum".into(), json!(choices));
    }
    ["minimum", "maximum"]
        .into_iter()
        .filter(|key| !argument[*key].is_null())
        .for_each(|key| {
            scalar.insert(key.into(), argument[key].clone());
        });
    if !argument["format"].is_null() {
        scalar.insert("format".into(), argument["format"].clone());
    }
    Value::Object(scalar)
}

fn property_json_schema(argument: &Value) -> Value {
    let scalar = scalar_json_schema(argument);
    let per_occurrence = &argument["values_per_occurrence"];
    let fixed_tuple = per_occurrence["min"].as_u64().filter(|&min| min > 1);
    let occurrence = match fixed_tuple {
        Some(count) if per_occurrence["max"].as_u64() == Some(count) => json!({
            "type": "array",
            "items": scalar,
            "minItems": count,
            "maxItems": count
        }),
        _ => scalar,
    };
    let positional_list = argument["kind"] == "positional";
    let mut property = if argument["repeatable"] == true || positional_list {
        json!({ "type": "array", "items": occurrence })
    } else {
        occurrence
    };
    if positional_list {
        property["minItems"] = json!(1);
    }
    if !argument["default"].is_null() {
        property["default"] = argument["default"].clone();
    }
    if let Some(help) = argument["help"].as_str() {
        property["description"] = json!(help);
    }
    property["x-stitchr-flag"] = match argument["long"].as_str() {
        Some(long) => json!(long),
        None => json!("<positional>"),
    };
    property
}

/// A standard JSON Schema (draft 2020-12) of a composition request, one property per argument.
pub(crate) fn json_schema() -> Value {
    let command = built_command();
    let arguments = command
        .get_arguments()
        .filter(|arg| !META_ARGUMENTS.contains(&arg.get_id().as_str()))
        .map(|arg| argument_schema(&command, arg))
        .collect::<Vec<_>>();
    let properties = arguments
        .iter()
        .filter_map(|argument| {
            Some((
                argument["name"].as_str()?.to_owned(),
                property_json_schema(argument),
            ))
        })
        .collect::<Map<_, _>>();
    let required = arguments
        .iter()
        .filter(|argument| argument["kind"] == "positional")
        .map(|argument| argument["name"].clone())
        .collect::<Vec<_>>();

    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": command.get_name(),
        "description": command.get_long_about().map(ToString::to_string),
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argument(name: &str) -> Value {
        schema()["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|arg| arg["name"] == name)
            .unwrap()
            .clone()
    }

    fn property(name: &str) -> Value {
        json_schema()["properties"][name].clone()
    }

    #[test]
    fn integer_arguments_carry_their_bounds() {
        assert_eq!(argument("columns")["minimum"], 1);
        assert_eq!(argument("columns")["maximum"], u32::MAX);
        assert_eq!(argument("spacing")["minimum"], 0);
        assert_eq!(argument("jpeg_quality")["minimum"], 1);
        assert_eq!(argument("jpeg_quality")["maximum"], 100);
        assert_eq!(argument("gradient_angle")["maximum"], 359);
        assert!(argument("layout")["minimum"].is_null());
    }

    #[test]
    fn arguments_describe_their_layouts_and_requirements() {
        assert_eq!(argument("columns")["applies_to_layouts"], json!(["grid"]));
        assert_eq!(argument("divider")["applies_to_layouts"], json!(["split"]));
        assert!(argument("layout")["applies_to_layouts"].is_null());
        assert_eq!(argument("border_width")["requires"], json!(["--label"]));
        assert_eq!(argument("layout")["requires"], json!([]));
        assert_eq!(argument("output")["ignored_with"], json!(["--print-spec"]));
    }

    #[test]
    fn conflicts_are_listed_in_both_directions() {
        assert_eq!(
            argument("background_color")["conflicts_with"],
            json!(["--background-gradient"])
        );
        assert_eq!(
            argument("background_gradient")["conflicts_with"],
            json!(["--background"])
        );
        assert_eq!(argument("spacing")["conflicts_with"], json!([]));
    }

    #[test]
    fn schema_lists_every_error_code_with_its_exit_code() {
        let codes = schema()["error_codes"].as_array().unwrap().clone();
        let exit = |code: &str| {
            codes.iter().find(|entry| entry["code"] == code).unwrap()["exit_code"].clone()
        };

        assert_eq!(codes.len(), error::ERROR_CODES.len());
        assert_eq!(exit("invalid_arguments"), 2);
        assert_eq!(exit("input_error"), 1);
        assert_eq!(exit("output_exists"), 1);
        assert!(codes
            .iter()
            .all(|entry| !entry["description"].as_str().unwrap().is_empty()));
    }

    #[test]
    fn schema_lists_warning_and_exit_codes() {
        let value = schema();

        assert_eq!(
            value["warning_codes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["code"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["option_ignored", "label_overflow", "missing_glyphs"]
        );
        assert_eq!(
            value["exit_codes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["code"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }

    #[test]
    fn json_schema_is_a_draft_2020_12_object_schema() {
        let value = json_schema();

        assert_eq!(
            value["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(value["type"], "object");
        assert_eq!(value["required"], json!(["images"]));
        assert_eq!(value["additionalProperties"], false);
    }

    #[test]
    fn json_schema_leaves_out_meta_flags() {
        let properties = json_schema()["properties"].clone();

        for name in META_ARGUMENTS {
            assert!(properties.get(name).is_none(), "{name}");
        }
        assert!(properties.get("print_spec").is_some());
        assert!(properties.get("strict").is_some());
    }

    #[test]
    fn json_schema_images_are_a_non_empty_path_list() {
        assert_eq!(
            property("images")["items"],
            json!({ "type": "string", "format": "path" })
        );
        assert_eq!(property("images")["type"], "array");
        assert_eq!(property("images")["minItems"], 1);
        assert_eq!(property("images")["x-stitchr-flag"], "<positional>");
    }

    #[test]
    fn json_schema_integers_have_bounds_and_defaults() {
        assert_eq!(
            property("columns"),
            json!({
                "type": "integer",
                "minimum": 1,
                "maximum": u32::MAX,
                "default": 2,
                "description": argument("columns")["help"],
                "x-stitchr-flag": "--columns"
            })
        );
    }

    #[test]
    fn json_schema_enums_list_their_values() {
        assert_eq!(
            property("layout")["enum"],
            json!(["grid", "horizontal", "vertical", "freeform", "split"])
        );
        assert_eq!(property("layout")["default"], "grid");
    }

    #[test]
    fn json_schema_pairs_are_fixed_length_arrays() {
        assert_eq!(property("background_gradient")["type"], "array");
        assert_eq!(property("background_gradient")["minItems"], 2);
        assert_eq!(property("background_gradient")["maxItems"], 2);
        assert_eq!(
            property("background_gradient")["items"],
            json!({ "type": "string", "format": "color" })
        );
    }

    #[test]
    fn json_schema_repeated_pairs_are_arrays_of_pairs() {
        assert_eq!(
            property("position")["items"],
            json!({
                "type": "array",
                "items": { "type": "integer", "minimum": 0, "maximum": u32::MAX },
                "minItems": 2,
                "maxItems": 2
            })
        );
        assert_eq!(property("label")["items"], json!({ "type": "string" }));
    }

    #[test]
    fn json_schema_booleans_default_to_false() {
        assert_eq!(property("strict")["type"], "boolean");
        assert_eq!(property("strict")["default"], false);
    }
}
