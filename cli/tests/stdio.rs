use serde_json::Value;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use stitchr_core::{export_image, load_image_from_bytes, DynamicImage, ExportFormat};
use tempfile::TempDir;

fn fixtures() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    export_image(
        &DynamicImage::new_rgb8(2, 1),
        directory.path().join("first.png"),
        ExportFormat::Png,
    )
    .unwrap();
    directory
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    stitchr_core::write_image(
        &DynamicImage::new_rgb8(width, height),
        &mut bytes,
        ExportFormat::Png,
        stitchr_core::DEFAULT_JPEG_QUALITY,
    )
    .unwrap();
    bytes
}

fn run(directory: &TempDir, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_stitchr"))
        .current_dir(directory.path())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The CLI may exit before reading stdin, so a broken pipe here is expected.
    let _ = child.stdin.take().unwrap().write_all(stdin);
    child.wait_with_output().unwrap()
}

fn json_error_code(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    value["error"]["code"].as_str().unwrap().to_owned()
}

#[test]
fn stdin_input_is_placed_like_a_file_and_named_dash() {
    let directory = fixtures();
    let output = run(
        &directory,
        &[
            "first.png",
            "-",
            "--layout",
            "horizontal",
            "--print-spec",
            "--json",
        ],
        &png_bytes(3, 4),
    );

    assert_eq!(output.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["spec"]["width"], 5);
    assert_eq!(value["spec"]["height"], 4);
    assert_eq!(value["spec"]["images"][1]["name"], "-");
}

#[test]
fn stdout_output_contains_only_the_encoded_image() {
    let directory = fixtures();
    for (format, magic) in [
        ("png", &[0x89, b'P', b'N', b'G'][..]),
        ("jpeg", &[0xff, 0xd8, 0xff][..]),
        ("webp", &b"RIFF"[..]),
    ] {
        let output = run(
            &directory,
            &[
                "-",
                "first.png",
                "--layout",
                "vertical",
                "-o",
                "-",
                "--format",
                format,
            ],
            &png_bytes(3, 4),
        );

        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert!(output.stdout.starts_with(magic));
        let image = load_image_from_bytes(&output.stdout).unwrap();
        assert_eq!((image.width(), image.height()), (3, 5));
    }
}

#[test]
fn format_overrides_the_output_extension() {
    let directory = fixtures();
    let output = run(
        &directory,
        &["first.png", "-o", "out.png", "--format", "jpeg", "--json"],
        &[],
    );

    assert_eq!(output.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["format"], "jpeg");
    assert!(std::fs::read(directory.path().join("out.png"))
        .unwrap()
        .starts_with(&[0xff, 0xd8, 0xff]));
}

#[test]
fn stdout_output_with_json_is_rejected() {
    let directory = fixtures();
    let output = run(
        &directory,
        &["first.png", "-o", "-", "--format", "png", "--json"],
        &[],
    );

    assert_eq!(json_error_code(&output), "invalid_arguments");
}

#[test]
fn stdout_output_without_format_is_rejected() {
    let directory = fixtures();
    let output = run(&directory, &["first.png", "-o", "-"], &[]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--format"));
}

#[test]
fn stdin_used_twice_is_rejected() {
    let directory = fixtures();
    let output = run(&directory, &["-", "-", "--json"], &png_bytes(1, 1));

    assert_eq!(json_error_code(&output), "invalid_arguments");
}

#[test]
fn undecodable_stdin_is_an_input_error() {
    let directory = fixtures();
    let output = run(&directory, &["-", "--json"], b"not an image");

    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "input_error");
}
