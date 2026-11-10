use crate::{Cli, SCHEMA_VERSION};
use clap::{Arg, ArgAction, CommandFactory, ValueHint};
use serde_json::{json, Value};

fn argument_schema(arg: &Arg) -> Value {
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

    json!({
        "name": arg.get_id().as_str(),
        "long": arg.get_long().map(|long| format!("--{long}")),
        "short": arg.get_short().map(|short| format!("-{short}")),
        "aliases": arg.get_visible_aliases().unwrap_or_default().into_iter().map(|alias| format!("--{alias}")).collect::<Vec<_>>(),
        "kind": if arg.is_positional() { "positional" } else { "option" },
        "type": value_type,
        "format": if path { Some("path") } else if color { Some("color") } else { None },
        "enum": choices,
        "default": default,
        "required": arg.is_required_set() || arg.is_positional(),
        "required_unless": if arg.is_positional() { vec!["--help-json", "--schema"] } else { vec![] },
        "repeatable": matches!(arg.get_action(), ArgAction::Append),
        "values_per_occurrence": {
            "min": arity.map(|range| range.min_values()).unwrap_or(0),
            "max": arity.and_then(|range| (range.max_values() != usize::MAX).then_some(range.max_values()))
        },
        "value_names": if boolean { None } else { arg.get_value_names().map(|names| names.iter().map(ToString::to_string).collect::<Vec<_>>()) },
        "help": arg.get_help().map(ToString::to_string)
    })
}

pub(crate) fn schema() -> Value {
    let mut command = Cli::command();
    command.build();
    let usage = command.render_usage().to_string();
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
        .map(argument_schema)
        .collect::<Vec<_>>();

    json!({
        "schema_version": SCHEMA_VERSION,
        "name": command.get_name(),
        "version": command.get_version(),
        "description": command.get_long_about().map(ToString::to_string),
        "usage": usage,
        "examples": examples,
        "arguments": arguments
    })
}
