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
        assert_eq!(
            value,
            json!({
                "schema_version": 1, "ok": true, "output": output,
                "width": 6, "height": 4, "format": format
            })
        );
        let image = stitchr_core::load_image(directory.path().join(output)).unwrap();
        assert_eq!((image.width(), image.height()), (6, 4));
    }
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
            vec!["first.png", "--output", "result.webp"],
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
