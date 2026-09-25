use std::process::{Command, Output};
use stitchr_core::{export_image, DynamicImage, ExportFormat};
use tempfile::TempDir;

fn run_cli(directory: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_stitchr"))
        .current_dir(directory.path())
        .args(args)
        .output()
        .unwrap()
}

fn assert_failure_contains(output: &Output, expected: &str) {
    assert!(
        !output.status.success(),
        "expected failure, stdout: {}, stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected),
        "expected stderr to contain {expected:?}, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_input(directory: &TempDir) {
    export_image(
        &DynamicImage::new_rgb8(1, 1),
        directory.path().join("input.png"),
        ExportFormat::Png,
    )
    .unwrap();
}

#[test]
fn unreadable_input_is_reported() {
    let directory = tempfile::tempdir().unwrap();
    let result = run_cli(&directory, &["missing.png"]);

    assert_failure_contains(&result, "cannot load missing.png");
}

#[test]
fn unwritable_output_is_reported() {
    let directory = tempfile::tempdir().unwrap();
    write_input(&directory);
    std::fs::create_dir(directory.path().join("blocked.png")).unwrap();

    let result = run_cli(&directory, &["input.png", "--output", "blocked.png"]);

    assert_failure_contains(&result, "cannot write blocked.png");
}

#[test]
fn unsupported_output_extension_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    write_input(&directory);

    let result = run_cli(&directory, &["input.png", "--output", "output.webp"]);

    assert_failure_contains(&result, "output filename must end in .png, .jpg, or .jpeg");
}
