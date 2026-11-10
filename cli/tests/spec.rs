use image::{Rgb, RgbImage};
use serde_json::{json, Value};
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};
use tempfile::TempDir;

fn fixtures() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    RgbImage::from_pixel(4, 3, Rgb([255, 0, 0]))
        .save(directory.path().join("red.png"))
        .unwrap();
    RgbImage::from_pixel(2, 3, Rgb([0, 0, 255]))
        .save(directory.path().join("blue.png"))
        .unwrap();
    directory
}

fn run(directory: &TempDir, args: &[&str]) -> Output {
    run_with_stdin(directory, args, &[])
}

fn run_with_stdin(directory: &TempDir, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_stitchr"))
        .current_dir(directory.path())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn json_of(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout: {} stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn printed_spec(directory: &TempDir, args: &[&str]) -> Value {
    let mut all = vec!["red.png", "blue.png", "--print-spec"];
    all.extend_from_slice(args);
    let output = run(directory, &all);
    assert_eq!(output.status.code(), Some(0));
    json_of(&output)
}

fn write_spec(directory: &TempDir, spec: &Value) {
    std::fs::write(directory.path().join("spec.json"), spec.to_string()).unwrap();
}

fn pixels(directory: &TempDir, name: &str) -> image::RgbaImage {
    image::open(directory.path().join(name)).unwrap().to_rgba8()
}

#[test]
fn printed_spec_renders_to_the_same_image_as_the_arguments() {
    let directory = fixtures();
    let args = [
        "--layout",
        "horizontal",
        "--spacing",
        "3",
        "--alignment",
        "center",
        "--background",
        "white",
        "--label",
        "A",
        "--label",
        "B",
    ];
    let spec = printed_spec(&directory, &args);
    write_spec(&directory, &spec);
    let mut direct = vec!["red.png", "blue.png", "-o", "direct.png"];
    direct.extend_from_slice(&args);
    assert_eq!(run(&directory, &direct).status.code(), Some(0));

    let output = run(&directory, &["--spec", "spec.json", "-o", "from-spec.png"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        pixels(&directory, "from-spec.png"),
        pixels(&directory, "direct.png")
    );
}

#[test]
fn split_spec_round_trips() {
    let directory = fixtures();
    RgbImage::from_pixel(4, 3, Rgb([0, 255, 0]))
        .save(directory.path().join("green.png"))
        .unwrap();
    let output = run(
        &directory,
        &[
            "red.png",
            "green.png",
            "--layout",
            "split",
            "--divider",
            "1",
            "--print-spec",
        ],
    );
    write_spec(&directory, &json_of(&output));
    assert_eq!(
        run(
            &directory,
            &[
                "red.png",
                "green.png",
                "--layout",
                "split",
                "--divider",
                "1",
                "-o",
                "direct.png"
            ]
        )
        .status
        .code(),
        Some(0)
    );

    assert_eq!(
        run(&directory, &["--spec", "spec.json", "-o", "from-spec.png"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        pixels(&directory, "from-spec.png"),
        pixels(&directory, "direct.png")
    );
}

#[test]
fn patched_position_moves_the_image() {
    let directory = fixtures();
    let mut spec = printed_spec(&directory, &["--layout", "horizontal"]);
    spec["width"] = json!(10);
    spec["images"][1]["position"] = json!({ "x": 8, "y": 0 });
    write_spec(&directory, &spec);

    let output = run(
        &directory,
        &["--json", "--spec", "spec.json", "-o", "out.png"],
    );

    assert_eq!(json_of(&output)["width"], 10);
    let image = pixels(&directory, "out.png");
    assert_eq!(image.get_pixel(5, 0).0, [0, 0, 0, 255]);
    assert_eq!(image.get_pixel(8, 0).0, [0, 0, 255, 255]);
}

#[test]
fn spec_from_stdin_renders() {
    let directory = fixtures();
    let spec = printed_spec(&directory, &["--layout", "vertical"]);

    let output = run_with_stdin(
        &directory,
        &["--json", "--spec", "-", "-o", "out.png"],
        spec.to_string().as_bytes(),
    );

    let value = json_of(&output);
    assert_eq!(value["ok"], true);
    assert_eq!(
        (value["width"].clone(), value["height"].clone()),
        (json!(4), json!(6))
    );
}

#[test]
fn spec_with_print_spec_echoes_the_normalized_spec() {
    let directory = fixtures();
    let spec = printed_spec(&directory, &[]);
    write_spec(&directory, &spec);

    let output = run(&directory, &["--spec", "spec.json", "--print-spec"]);

    assert_eq!(json_of(&output), spec);
}

#[test]
fn relative_spec_paths_resolve_against_the_working_directory() {
    let directory = fixtures();
    let mut spec = printed_spec(&directory, &[]);
    spec["images"][0]["path"] = json!("red.png");
    spec["images"][1]["path"] = json!("./blue.png");
    write_spec(&directory, &spec);

    let output = run(
        &directory,
        &["--json", "--spec", "spec.json", "-o", "out.png"],
    );

    assert_eq!(json_of(&output)["ok"], true);
}

#[test]
fn changed_input_size_is_reported() {
    let directory = fixtures();
    let spec = printed_spec(&directory, &[]);
    write_spec(&directory, &spec);
    RgbImage::new(9, 9)
        .save(directory.path().join("blue.png"))
        .unwrap();

    let output = run(
        &directory,
        &["--json", "--spec", "spec.json", "-o", "out.png"],
    );

    assert_eq!(output.status.code(), Some(1));
    let value = json_of(&output);
    assert_eq!(value["error"]["code"], "image_size_mismatch");
    assert_eq!(value["error"]["details"]["index"], 1);
    assert_eq!(value["error"]["details"]["width"], 9);
    assert!(!directory.path().join("out.png").exists());
}

#[test]
fn layout_options_with_a_spec_are_reported_as_ignored() {
    let directory = fixtures();
    write_spec(&directory, &printed_spec(&directory, &[]));

    let output = run(
        &directory,
        &[
            "--json",
            "--spec",
            "spec.json",
            "--spacing",
            "9",
            "--print-spec",
        ],
    );

    assert_eq!(
        json_of(&output)["warnings"],
        json!([{
            "code": "option_ignored",
            "option": "--spacing",
            "reason": "--spacing is not used with --spec"
        }])
    );
}

#[test]
fn spec_and_positional_inputs_conflict() {
    let directory = fixtures();
    write_spec(&directory, &printed_spec(&directory, &[]));

    let output = run(&directory, &["--json", "red.png", "--spec", "spec.json"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        json_of(&output)["error"]["details"]["kind"],
        "argument_conflict"
    );
}

#[test]
fn invalid_spec_exits_with_two() {
    let directory = fixtures();
    std::fs::write(directory.path().join("spec.json"), "{\"images\": []").unwrap();

    let output = run(&directory, &["--json", "--spec", "spec.json"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json_of(&output)["error"]["code"], "invalid_spec");
}

#[test]
fn spec_inputs_cannot_overwrite_themselves_without_force() {
    let directory = fixtures();
    write_spec(&directory, &printed_spec(&directory, &[]));

    let output = run(
        &directory,
        &["--json", "--spec", "spec.json", "-o", "red.png"],
    );

    assert_eq!(
        json_of(&output)["error"]["code"],
        "output_conflicts_with_input"
    );
}
