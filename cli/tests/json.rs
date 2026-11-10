use serde_json::{json, Value};
use std::process::{Command, Output};
use stitchr_core::{export_image, DynamicImage, ExportFormat};
use tempfile::TempDir;

fn fixtures() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    export_image(
        &DynamicImage::new_rgb8(2, 1),
        directory.path().join("first.png"),
        ExportFormat::Png,
    )
    .unwrap();
    export_image(
        &DynamicImage::new_rgb8(1, 2),
        directory.path().join("second.png"),
        ExportFormat::Png,
    )
    .unwrap();
    directory
}

fn run(directory: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_stitchr"))
        .current_dir(directory.path())
        .args(args)
        .output()
        .unwrap()
}

fn response(output: &Output, exit_code: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit_code));
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["ok"], exit_code == 0);
    value
}

#[test]
fn render_reports_written_file_dimensions_and_normalized_format() {
    let directory = fixtures();
    for (output, format) in [
        ("combined.PNG", "png"),
        ("combined.JPG", "jpeg"),
        ("combined.jpeg", "jpeg"),
        ("combined.WebP", "webp"),
    ] {
        let value = response(
            &run(
                &directory,
                &[
                    "first.png",
                    "second.png",
                    "--layout",
                    "horizontal",
                    "--spacing",
                    "1",
                    "--output",
                    output,
                    "--json",
                ],
            ),
            0,
        );
        let output_abs = std::fs::canonicalize(directory.path())
            .unwrap()
            .join(output);
        assert_eq!(
            value,
            json!({
                "schema_version": 1, "ok": true, "output": output,
                "output_abs": output_abs.to_string_lossy(),
                "width": 6, "height": 4, "format": format, "scale": 1.0, "warnings": []
            })
        );
        let image = stitchr_core::load_image(directory.path().join(output)).unwrap();
        assert_eq!((image.width(), image.height()), (6, 4));
    }
}

#[test]
fn invalid_jpeg_quality_is_an_argument_error() {
    let directory = fixtures();
    let value = response(
        &run(
            &directory,
            &[
                "first.png",
                "--jpeg-quality",
                "0",
                "-o",
                "out.jpg",
                "--json",
            ],
        ),
        2,
    );
    assert_eq!(value["error"]["code"], "invalid_arguments");
}

#[test]
fn processing_failures_have_stable_codes_and_no_output_file() {
    let directory = fixtures();
    std::fs::write(directory.path().join("corrupt.png"), "not an image").unwrap();
    std::fs::create_dir(directory.path().join("blocked.png")).unwrap();
    for (args, code) in [
        (vec!["missing.png"], "input_error"),
        (vec!["corrupt.png"], "input_error"),
        (
            vec!["first.png", "--output", "result.bmp"],
            "unsupported_output_format",
        ),
        (vec!["first.png", "--output", "blocked.png"], "output_error"),
        (
            vec!["first.png", "second.png", "--layout", "split"],
            "split_size_mismatch",
        ),
        (
            vec![
                "first.png",
                "second.png",
                "--layout",
                "freeform",
                "--position",
                "0",
                "0",
            ],
            "freeform_position_count",
        ),
        (
            vec!["first.png", "second.png", "--label", "Only one"],
            "label_count_mismatch",
        ),
        (vec!["first.png", "--spacing", "10000"], "canvas_too_large"),
        (
            vec!["first.png", "--spacing", "4294967295"],
            "canvas_too_large",
        ),
    ] {
        let mut args = args;
        args.insert(0, "--json");
        let value = response(&run(&directory, &args), 1);
        assert_eq!(value["error"]["code"], code, "{args:?}");
        assert!(!value["error"]["message"].as_str().unwrap().is_empty());
        assert!(!directory.path().join("stitched.png").exists());
    }
}

#[test]
fn argument_failures_are_json_even_when_parsing_stops_before_the_flag() {
    let directory = fixtures();
    for args in [
        vec!["--json"],
        vec!["--unknown", "--json"],
        vec!["first.png", "--columns", "0", "--json"],
        vec!["first.png", "--spacing", "-1", "--json"],
        vec!["first.png", "--background", "notacolor", "--json"],
        vec!["first.png", "--layout", "bogus", "--json"],
        vec!["first.png", "--position", "0", "--json"],
        vec!["first.png", "--json=true"],
    ] {
        let value = response(&run(&directory, &args), 2);
        assert_eq!(value["error"]["code"], "invalid_arguments", "{args:?}");
    }
}

#[test]
fn negative_spacing_reports_the_bound_in_json() {
    let directory = fixtures();
    let value = response(
        &run(&directory, &["--json", "first.png", "--spacing", "-5"]),
        2,
    );
    let message = value["error"]["message"].as_str().unwrap();

    assert_eq!(value["error"]["code"], "invalid_arguments");
    assert!(message.contains("must be ≥ 0"), "{message}");
    assert!(!message.contains("-- -5"), "{message}");
}

#[test]
fn repeated_background_gradient_is_an_argument_error() {
    let directory = fixtures();
    let value = response(
        &run(
            &directory,
            &[
                "--json",
                "first.png",
                "--background-gradient",
                "red",
                "blue",
                "--background-gradient",
                "green",
                "white",
                "--print-spec",
            ],
        ),
        2,
    );

    assert_eq!(value["error"]["code"], "invalid_arguments");
}

#[test]
fn spec_and_schema_modes_are_versioned_and_can_be_wrapped() {
    let directory = fixtures();
    let plain = run(&directory, &["first.png", "--print-spec"]);
    assert!(plain.status.success());
    let spec: Value = serde_json::from_slice(&plain.stdout).unwrap();
    assert_eq!(spec["schema_version"], 1);
    let wrapped = response(
        &run(&directory, &["--json", "first.png", "--print-spec"]),
        0,
    );
    assert_eq!(wrapped["spec"], spec);
    assert!(!directory.path().join("stitched.png").exists());

    for flag in ["--help-json", "--schema"] {
        let plain = run(&directory, &[flag]);
        let schema: Value = serde_json::from_slice(&plain.stdout).unwrap();
        assert_eq!(schema["schema_version"], 1);
        let wrapped = response(&run(&directory, &["--json", flag, "--print-spec"]), 0);
        assert_eq!(wrapped["schema"], schema);
        assert!(schema["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["long"] == "--json"));
    }
}

#[test]
fn help_and_version_stay_json_when_requested() {
    let directory = fixtures();
    for flag in ["--help", "-h"] {
        let value = response(&run(&directory, &["--json", flag]), 0);
        assert!(value["help"].as_str().unwrap().contains("--json"));
    }
    let value = response(&run(&directory, &["--json", "--version"]), 0);
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn flag_values_and_paths_after_separator_do_not_enable_json() {
    let directory = fixtures();
    for args in [vec!["--", "--json"], vec!["first.png", "--output=--json"]] {
        let output = run(&directory, &args);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("stitchr: "));
    }
}

#[test]
fn split_size_mismatch_details_name_the_offending_input() {
    let directory = fixtures();
    let value = response(
        &run(
            &directory,
            &[
                "--json",
                "first.png",
                "first.png",
                "second.png",
                "--layout",
                "split",
            ],
        ),
        1,
    );

    assert_eq!(
        value["error"]["details"],
        json!({
            "index": 2, "path": "second.png", "width": 1, "height": 2,
            "expected": { "width": 2, "height": 1 }
        })
    );
}

#[test]
fn input_error_details_give_index_and_path() {
    let directory = fixtures();
    let value = response(&run(&directory, &["--json", "first.png", "missing.png"]), 1);

    assert_eq!(
        value["error"]["details"],
        json!({ "index": 1, "path": "missing.png" })
    );
}

#[test]
fn canvas_too_large_details_report_size_and_limit() {
    let directory = fixtures();
    let value = response(
        &run(&directory, &["--json", "first.png", "--spacing", "10000"]),
        1,
    );

    assert_eq!(
        value["error"]["details"],
        json!({
            "width": 20002, "height": 20001,
            "bytes": 20002_u64 * 20001 * 4, "limit_bytes": 536_870_912
        })
    );
}

#[test]
fn argument_error_details_describe_the_option() {
    let directory = fixtures();
    let value = response(
        &run(&directory, &["--json", "first.png", "--columns", "0"]),
        2,
    );
    let details = &value["error"]["details"];

    assert_eq!(details["kind"], "value_validation");
    assert_eq!(details["option"], "--columns");
    assert_eq!(details["value"], "0");
}

#[test]
fn unsupported_output_details_list_extensions() {
    let directory = fixtures();
    let value = response(
        &run(&directory, &["--json", "first.png", "-o", "out.bmp"]),
        1,
    );

    assert_eq!(
        value["error"]["details"],
        json!({ "path": "out.bmp", "supported_extensions": ["png", "jpg", "jpeg", "webp"] })
    );
}

#[test]
fn freeform_position_count_details_report_counts() {
    let directory = fixtures();
    let value = response(
        &run(
            &directory,
            &[
                "--json",
                "first.png",
                "second.png",
                "--layout",
                "freeform",
                "--position",
                "0",
                "0",
            ],
        ),
        1,
    );

    assert_eq!(
        value["error"]["details"],
        json!({ "expected": 2, "actual": 1 })
    );
}

#[test]
fn output_that_is_an_input_is_refused_without_force() {
    let directory = fixtures();
    let before = std::fs::read(directory.path().join("first.png")).unwrap();
    let value = response(
        &run(
            &directory,
            &["--json", "first.png", "second.png", "-o", "first.png"],
        ),
        1,
    );

    assert_eq!(value["error"]["code"], "output_conflicts_with_input");
    assert_eq!(
        value["error"]["details"],
        json!({ "path": "first.png", "index": 0, "input": "first.png" })
    );
    assert_eq!(
        std::fs::read(directory.path().join("first.png")).unwrap(),
        before
    );
}

#[test]
fn force_allows_overwriting_an_input() {
    let directory = fixtures();
    let value = response(
        &run(
            &directory,
            &[
                "--json",
                "first.png",
                "second.png",
                "--layout",
                "horizontal",
                "-o",
                "first.png",
                "--force",
            ],
        ),
        0,
    );

    assert_eq!(value["width"], 3);
    let image = stitchr_core::load_image(directory.path().join("first.png")).unwrap();
    assert_eq!((image.width(), image.height()), (3, 2));
}

#[test]
fn print_spec_does_not_check_the_destination() {
    let directory = fixtures();
    let value = response(
        &run(
            &directory,
            &["--json", "first.png", "-o", "first.png", "--print-spec"],
        ),
        0,
    );

    assert_eq!(value["spec"]["width"], 2);
}

#[test]
fn no_clobber_refuses_an_existing_output() {
    let directory = fixtures();
    std::fs::write(directory.path().join("out.png"), "keep").unwrap();
    let value = response(
        &run(
            &directory,
            &["--json", "first.png", "-o", "out.png", "--no-clobber"],
        ),
        1,
    );

    assert_eq!(value["error"]["code"], "output_exists");
    assert_eq!(value["error"]["details"], json!({ "path": "out.png" }));
    assert_eq!(
        std::fs::read(directory.path().join("out.png")).unwrap(),
        b"keep"
    );
}

#[test]
fn no_clobber_writes_a_new_output() {
    let directory = fixtures();
    let value = response(
        &run(
            &directory,
            &["--json", "first.png", "-o", "new.png", "--no-clobber"],
        ),
        0,
    );

    assert_eq!(value["output"], "new.png");
    assert!(directory.path().join("new.png").exists());
}

#[test]
fn mkdir_creates_missing_parent_directories() {
    let directory = fixtures();
    let value = response(
        &run(
            &directory,
            &["--json", "first.png", "-o", "a/b/out.png", "--mkdir"],
        ),
        0,
    );

    assert_eq!(value["output"], "a/b/out.png");
    assert!(value["output_abs"]
        .as_str()
        .unwrap()
        .ends_with("a/b/out.png"));
    assert!(directory.path().join("a/b/out.png").exists());
}

#[test]
fn missing_parent_directory_without_mkdir_is_an_output_error() {
    let directory = fixtures();
    let value = response(
        &run(&directory, &["--json", "first.png", "-o", "a/b/out.png"]),
        1,
    );

    assert_eq!(value["error"]["code"], "output_error");
    assert_eq!(value["error"]["details"], json!({ "path": "a/b/out.png" }));
}

#[test]
fn json_schema_format_is_available_through_both_flag_names() {
    let directory = fixtures();
    for flag in ["--help-json=jsonschema", "--schema=jsonschema"] {
        let plain = run(&directory, &[flag]);
        assert_eq!(plain.status.code(), Some(0));
        let schema: Value = serde_json::from_slice(&plain.stdout).unwrap();
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(schema["properties"]["columns"]["minimum"], 1);

        let wrapped = response(&run(&directory, &["--json", flag]), 0);
        assert_eq!(wrapped["schema"], schema);
    }
}

#[test]
fn schema_flag_without_equals_does_not_swallow_an_input_path() {
    let directory = fixtures();
    let value = response(&run(&directory, &["--json", "--schema", "first.png"]), 0);

    assert_eq!(value["schema"]["name"], "stitchr");
    assert!(!directory.path().join("stitched.png").exists());
}

#[test]
fn unknown_schema_format_is_an_argument_error() {
    let directory = fixtures();
    let value = response(&run(&directory, &["--json", "--schema=yaml"]), 2);

    assert_eq!(value["error"]["code"], "invalid_arguments");
    assert_eq!(
        value["error"]["details"]["allowed"],
        json!(["stitchr", "jsonschema"])
    );
}

#[test]
fn stitchr_schema_lists_error_codes_and_bounds() {
    let directory = fixtures();
    let value = response(&run(&directory, &["--json", "--schema"]), 0);
    let schema = &value["schema"];

    assert!(schema["error_codes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["code"] == "split_size_mismatch" && entry["exit_code"] == 1));
    let quality = schema["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|arg| arg["name"] == "jpeg_quality")
        .unwrap();
    assert_eq!(quality["minimum"], 1);
    assert_eq!(quality["maximum"], 100);
    assert_eq!(quality["applies_to_formats"], json!(["jpeg"]));
}
