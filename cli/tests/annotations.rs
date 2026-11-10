use image::{Rgb, RgbImage};
use serde_json::{json, Value};
use std::process::{Command, Output};
use tempfile::TempDir;

const WHITE: [u8; 3] = [255, 255, 255];
const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];

fn fixtures() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    RgbImage::from_pixel(60, 40, Rgb(WHITE))
        .save(directory.path().join("a.png"))
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

fn json_run(directory: &TempDir, args: &[&str]) -> (Option<i32>, Value) {
    let mut all = vec!["--json"];
    all.extend_from_slice(args);
    let output = run(directory, &all);
    (
        output.status.code(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

fn pixel(directory: &TempDir, name: &str, x: u32, y: u32) -> [u8; 3] {
    image::open(directory.path().join(name))
        .unwrap()
        .to_rgb8()
        .get_pixel(x, y)
        .0
}

#[test]
fn boxes_and_arrows_are_drawn_on_top_of_the_output() {
    let directory = fixtures();
    let (code, value) = json_run(
        &directory,
        &[
            "a.png", "--box", "5", "5", "20", "10", "--arrow", "30", "30", "55", "30", "-o",
            "out.png",
        ],
    );

    assert_eq!(code, Some(0), "{value}");
    assert_eq!(value["warnings"], json!([]));
    assert_eq!(pixel(&directory, "out.png", 5, 5), RED);
    assert_eq!(pixel(&directory, "out.png", 15, 10), WHITE);
    assert_eq!(pixel(&directory, "out.png", 24, 14), RED);
    assert_eq!(pixel(&directory, "out.png", 40, 30), RED);
    assert_eq!(pixel(&directory, "out.png", 33, 35), WHITE);
}

#[test]
fn annotation_color_and_width_style_every_shape() {
    let directory = fixtures();
    let (code, _) = json_run(
        &directory,
        &[
            "a.png",
            "--box",
            "0",
            "0",
            "60",
            "40",
            "--annotation-color",
            "blue",
            "--annotation-width",
            "6",
            "-o",
            "out.png",
        ],
    );

    assert_eq!(code, Some(0));
    assert_eq!(pixel(&directory, "out.png", 5, 20), BLUE);
    assert_eq!(pixel(&directory, "out.png", 6, 20), WHITE);
}

#[test]
fn printed_spec_lists_annotations_and_renders_them_again() {
    let directory = fixtures();
    let (_, printed) = json_run(
        &directory,
        &["a.png", "--arrow", "1", "2", "30", "20", "--print-spec"],
    );

    assert_eq!(
        printed["spec"]["annotations"],
        json!([{
            "type": "arrow",
            "from": { "x": 1, "y": 2 },
            "to": { "x": 30, "y": 20 },
            "color": { "red": 255, "green": 0, "blue": 0 },
            "line_width": 4
        }])
    );

    let mut spec = printed["spec"].clone();
    spec["annotations"] = json!([{
        "type": "box", "x": 10, "y": 10, "width": 10, "height": 10,
        "color": { "red": 0, "green": 0, "blue": 255 }, "line_width": 1
    }]);
    std::fs::write(directory.path().join("spec.json"), spec.to_string()).unwrap();
    let (code, value) = json_run(&directory, &["--spec", "spec.json", "-o", "again.png"]);

    assert_eq!(code, Some(0), "{value}");
    assert_eq!(pixel(&directory, "again.png", 10, 10), BLUE);
    assert_eq!(pixel(&directory, "again.png", 2, 3), WHITE);
}

#[test]
fn shapes_past_the_canvas_warn_and_fail_under_strict() {
    let directory = fixtures();
    let (code, value) = json_run(
        &directory,
        &["a.png", "--box", "50", "30", "20", "20", "--print-spec"],
    );

    assert_eq!(code, Some(0));
    assert_eq!(value["warnings"][0]["code"], "annotation_clipped");
    assert_eq!(value["warnings"][0]["visible"], true);

    let (code, value) = json_run(
        &directory,
        &[
            "a.png", "--strict", "--arrow", "70", "0", "90", "10", "-o", "out.png",
        ],
    );

    assert_eq!(code, Some(2));
    assert_eq!(value["error"]["code"], "invalid_arguments");
    assert_eq!(value["error"]["details"]["warnings"][0]["visible"], false);
    assert!(!directory.path().join("out.png").exists());
}

#[test]
fn degenerate_shapes_are_rejected_before_writing() {
    let directory = fixtures();
    let (code, value) = json_run(
        &directory,
        &[
            "a.png", "--box", "0", "0", "5", "5", "--arrow", "3", "3", "3", "3", "-o", "out.png",
        ],
    );

    assert_eq!(code, Some(2));
    assert_eq!(
        value["error"]["details"],
        json!({
            "annotation_index": 1,
            "reason": "arrow start and end must differ",
            "option": "--arrow"
        })
    );
    assert_eq!(value["error"]["code"], "invalid_annotation");
    assert!(!directory.path().join("out.png").exists());
}

#[test]
fn degenerate_spec_annotations_are_rejected() {
    let directory = fixtures();
    let (_, printed) = json_run(&directory, &["a.png", "--print-spec"]);
    let mut spec = printed["spec"].clone();
    spec["annotations"] = json!([{
        "type": "box", "x": 0, "y": 0, "width": 0, "height": 4,
        "color": { "red": 0, "green": 0, "blue": 0 }, "line_width": 1
    }]);
    std::fs::write(directory.path().join("spec.json"), spec.to_string()).unwrap();

    let (code, value) = json_run(&directory, &["--spec", "spec.json", "-o", "out.png"]);

    assert_eq!(code, Some(2));
    assert_eq!(value["error"]["code"], "invalid_annotation");
    assert_eq!(value["error"]["details"]["annotation_index"], 0);
    assert!(value["error"]["details"].get("path").is_none());
}
