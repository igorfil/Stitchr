use serde_json::{json, Value};
use std::process::{Command, Output};
use stitchr_core::{export_image, DynamicImage, ExportFormat};
use tempfile::TempDir;

fn fixtures() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    for name in ["a.png", "b.png"] {
        export_image(
            &DynamicImage::new_rgb8(8, 6),
            directory.path().join(name),
            ExportFormat::Png,
        )
        .unwrap();
    }
    directory
}

fn run(directory: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_stitchr"))
        .current_dir(directory.path())
        .args(args)
        .output()
        .unwrap()
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn json_render_lists_warnings_and_keeps_stderr_empty() {
    let directory = fixtures();
    let output = run(
        &directory,
        &[
            "--json",
            "a.png",
            "b.png",
            "--layout",
            "split",
            "--spacing",
            "40",
            "-o",
            "out.png",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        stdout_json(&output)["warnings"],
        json!([{
            "code": "option_ignored",
            "option": "--spacing",
            "reason": "--spacing is not used by the split layout"
        }])
    );
    assert!(directory.path().join("out.png").exists());
}

#[test]
fn human_render_prints_warnings_to_stderr_and_the_path_to_stdout() {
    let directory = fixtures();
    let output = run(
        &directory,
        &[
            "a.png",
            "--layout",
            "split",
            "--divider",
            "2",
            "--columns",
            "4",
            "-o",
            "out.png",
        ],
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "out.png\n");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "stitchr: warning: --columns is not used by the split layout\n"
    );
}

#[test]
fn human_print_spec_keeps_stdout_pure_json() {
    let directory = fixtures();
    let output = run(
        &directory,
        &["a.png", "--print-spec", "--jpeg-quality", "5"],
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout_json(&output)["width"], 8);
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "stitchr: warning: --jpeg-quality is not used with --print-spec\n"
    );
}

#[test]
fn stdout_image_output_still_reports_warnings_on_stderr() {
    let directory = fixtures();
    let output = run(
        &directory,
        &["a.png", "--divider", "1", "-o", "-", "--format", "png"],
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.starts_with(b"\x89PNG"));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "stitchr: warning: --divider is not used by the grid layout\n"
    );
}

#[test]
fn strict_fails_with_exit_two_and_writes_nothing() {
    let directory = fixtures();
    let output = run(
        &directory,
        &[
            "--json",
            "--strict",
            "a.png",
            "--gradient-angle",
            "90",
            "-o",
            "out.png",
        ],
    );

    assert_eq!(output.status.code(), Some(2));
    let value = stdout_json(&output);
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "invalid_arguments");
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .contains("--gradient-angle is only used with --background-gradient"));
    assert!(!directory.path().join("out.png").exists());
}

#[test]
fn label_warnings_reach_json_output() {
    let directory = fixtures();
    let output = run(
        &directory,
        &["--json", "a.png", "--label", "Long label", "--print-spec"],
    );
    let warnings = stdout_json(&output)["warnings"].clone();

    assert_eq!(warnings[0]["code"], "label_overflow");
    assert_eq!(warnings[0]["index"], 0);
    assert_eq!(warnings[0]["label"], "Long label");
    assert_eq!(warnings[0]["available_width"], 12);
}

#[test]
fn jpeg_format_into_a_png_file_warns() {
    let directory = fixtures();
    let output = run(
        &directory,
        &["--json", "a.png", "--format", "jpeg", "-o", "out.png"],
    );
    let value = stdout_json(&output);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(value["format"], "jpeg");
    assert_eq!(value["warnings"][0]["code"], "format_mismatch");
    assert!(std::fs::read(directory.path().join("out.png"))
        .unwrap()
        .starts_with(&[0xff, 0xd8, 0xff]));
}

#[test]
fn strict_refuses_a_format_mismatch() {
    let directory = fixtures();
    let output = run(
        &directory,
        &[
            "--json", "--strict", "a.png", "--format", "jpeg", "-o", "out.png",
        ],
    );

    assert_eq!(output.status.code(), Some(2));
    assert!(!directory.path().join("out.png").exists());
}
