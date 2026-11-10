use image::{Rgb, RgbImage};
use serde_json::{json, Value};
use std::process::{Command, Output};
use tempfile::TempDir;

fn save(directory: &TempDir, name: &str, width: u32, height: u32, color: [u8; 3]) {
    RgbImage::from_pixel(width, height, Rgb(color))
        .save(directory.path().join(name))
        .unwrap();
}

fn run(directory: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_stitchr"))
        .current_dir(directory.path())
        .args(args)
        .output()
        .unwrap()
}

fn json_run(directory: &TempDir, args: &[&str]) -> Value {
    let mut all = vec!["--json"];
    all.extend_from_slice(args);
    serde_json::from_slice(&run(directory, &all).stdout).unwrap()
}

fn sizes(spec: &Value) -> Vec<(u64, u64)> {
    spec["images"]
        .as_array()
        .unwrap()
        .iter()
        .map(|image| {
            (
                image["width"].as_u64().unwrap(),
                image["height"].as_u64().unwrap(),
            )
        })
        .collect()
}

fn screenshots() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    save(&directory, "wide.png", 160, 80, [255, 0, 0]);
    save(&directory, "tall.png", 30, 60, [0, 0, 255]);
    directory
}

#[test]
fn fit_height_min_gives_a_horizontal_stack_equal_heights() {
    let directory = screenshots();
    let value = json_run(
        &directory,
        &[
            "wide.png",
            "tall.png",
            "--layout",
            "horizontal",
            "--fit-height",
            "min",
            "--print-spec",
        ],
    );

    assert_eq!(sizes(&value["spec"]), [(120, 60), (30, 60)]);
    assert_eq!(value["spec"]["width"], 150);
    assert_eq!(value["spec"]["height"], 60);
}

#[test]
fn fit_height_renders_the_shrunk_images() {
    let directory = screenshots();
    let value = json_run(
        &directory,
        &[
            "wide.png",
            "tall.png",
            "--layout",
            "horizontal",
            "--fit-height",
            "40",
            "-o",
            "out.png",
        ],
    );

    assert_eq!(
        (value["width"].clone(), value["height"].clone()),
        (json!(100), json!(40))
    );
    let image = image::open(directory.path().join("out.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(image.get_pixel(40, 20).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(90, 20).0, [0, 0, 255, 255]);
}

#[test]
fn fit_height_larger_than_an_image_warns_and_leaves_it() {
    let directory = screenshots();
    let value = json_run(
        &directory,
        &[
            "wide.png",
            "tall.png",
            "--layout",
            "horizontal",
            "--fit-height",
            "70",
            "--print-spec",
        ],
    );

    assert_eq!(sizes(&value["spec"]), [(140, 70), (30, 60)]);
    assert_eq!(value["warnings"][0]["code"], "fit_not_applied");
    assert_eq!(value["warnings"][0]["index"], 1);
}

#[test]
fn strict_rejects_a_fit_that_does_not_apply() {
    let directory = screenshots();
    let output = run(
        &directory,
        &[
            "--json",
            "--strict",
            "wide.png",
            "tall.png",
            "--fit-height",
            "70",
            "-o",
            "out.png",
        ],
    );

    assert_eq!(output.status.code(), Some(2));
    assert!(!directory.path().join("out.png").exists());
}

#[test]
fn fit_width_min_gives_a_vertical_stack_equal_widths() {
    let directory = screenshots();
    let value = json_run(
        &directory,
        &[
            "wide.png",
            "tall.png",
            "--layout",
            "vertical",
            "--fit-width",
            "min",
            "--print-spec",
        ],
    );

    assert_eq!(sizes(&value["spec"]), [(30, 15), (30, 60)]);
}

#[test]
fn invalid_fit_targets_are_argument_errors() {
    let directory = screenshots();
    for value in ["0", "-3", "max", "1.5"] {
        let output = run(&directory, &["--json", "wide.png", "--fit-height", value]);
        assert_eq!(output.status.code(), Some(2), "{value}");
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            response["error"]["details"]["reason"], "must be a positive integer or min",
            "{value}"
        );
    }
}

#[test]
fn split_cover_crops_inputs_to_the_smallest_common_size() {
    let directory = tempfile::tempdir().unwrap();
    save(&directory, "a.png", 40, 20, [255, 0, 0]);
    save(&directory, "b.png", 30, 30, [0, 0, 255]);
    let value = json_run(
        &directory,
        &[
            "a.png", "b.png", "--layout", "split", "--fit", "cover", "-o", "out.png",
        ],
    );

    assert_eq!(
        (value["width"].clone(), value["height"].clone()),
        (json!(30), json!(20))
    );
    let image = image::open(directory.path().join("out.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(image.get_pixel(0, 19).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(29, 0).0, [0, 0, 255, 255]);
}

#[test]
fn split_contain_pads_with_the_background() {
    let directory = tempfile::tempdir().unwrap();
    save(&directory, "a.png", 40, 20, [255, 0, 0]);
    save(&directory, "b.png", 40, 40, [0, 0, 255]);
    let value = json_run(
        &directory,
        &[
            "a.png",
            "b.png",
            "--layout",
            "split",
            "--fit",
            "contain",
            "--background",
            "white",
            "--print-spec",
        ],
    );

    assert_eq!(sizes(&value["spec"]), [(40, 20), (40, 20)]);
    assert_eq!(value["spec"]["images"][1]["fit"], "contain");
    let output = run(
        &directory,
        &[
            "a.png",
            "b.png",
            "--layout",
            "split",
            "--fit",
            "contain",
            "--background",
            "white",
            "-o",
            "out.png",
        ],
    );
    assert_eq!(output.status.code(), Some(0));
    let image = image::open(directory.path().join("out.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(image.get_pixel(39, 0).0, [255, 255, 255, 255]);
    assert_eq!(image.get_pixel(25, 10).0, [0, 0, 255, 255]);
}

#[test]
fn split_without_fit_still_requires_equal_sizes() {
    let directory = screenshots();
    let value = json_run(&directory, &["wide.png", "tall.png", "--layout", "split"]);

    assert_eq!(value["error"]["code"], "split_size_mismatch");
}

#[test]
fn max_width_shrinks_images_but_keeps_spacing() {
    let directory = screenshots();
    let value = json_run(
        &directory,
        &[
            "wide.png",
            "tall.png",
            "--layout",
            "horizontal",
            "--spacing",
            "10",
            "--max-width",
            "125",
            "--print-spec",
        ],
    );
    let spec = &value["spec"];

    assert!(spec["width"].as_u64().unwrap() <= 125);
    assert!(spec["scale"].as_f64().unwrap() < 1.0);
    assert_eq!(spec["images"][0]["position"]["x"], 10);
    let first_right = spec["images"][0]["position"]["x"].as_u64().unwrap()
        + spec["images"][0]["width"].as_u64().unwrap();
    assert_eq!(
        spec["images"][1]["position"]["x"].as_u64().unwrap() - first_right,
        10
    );
}

#[test]
fn max_size_already_satisfied_changes_nothing() {
    let directory = screenshots();
    let bounded = json_run(
        &directory,
        &[
            "wide.png",
            "tall.png",
            "--max-width",
            "5000",
            "--max-height",
            "5000",
            "--print-spec",
        ],
    );
    let plain = json_run(&directory, &["wide.png", "tall.png", "--print-spec"]);

    assert_eq!(bounded["spec"], plain["spec"]);
    assert!(bounded["spec"].get("scale").is_none());
}

#[test]
fn max_height_keeps_labels_at_full_size() {
    let directory = screenshots();
    let value = json_run(
        &directory,
        &[
            "wide.png",
            "--label",
            "",
            "--border-width",
            "4",
            "--max-height",
            "48",
            "-o",
            "out.png",
        ],
    );

    assert_eq!(value["height"], 48);
    assert_eq!(value["width"], 88);
}

#[test]
fn max_width_scales_freeform_positions() {
    let directory = screenshots();
    let value = json_run(
        &directory,
        &[
            "wide.png",
            "tall.png",
            "--layout",
            "freeform",
            "--position",
            "0",
            "0",
            "--position",
            "160",
            "0",
            "--max-width",
            "95",
            "--print-spec",
        ],
    );
    let spec = &value["spec"];

    assert!(spec["width"].as_u64().unwrap() <= 95);
    assert!(spec["images"][1]["position"]["x"].as_u64().unwrap() < 160);
}

#[test]
fn max_size_smaller_than_the_spacing_cannot_fit() {
    let directory = screenshots();
    let output = run(
        &directory,
        &[
            "--json",
            "wide.png",
            "tall.png",
            "--spacing",
            "50",
            "--max-width",
            "100",
        ],
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(value["error"]["code"], "canvas_cannot_fit");
    assert_eq!(value["error"]["details"]["max_width"], 100);
    assert_eq!(value["error"]["details"]["width"], 152);
}

#[test]
fn fit_and_max_size_combine_into_one_resize() {
    let directory = screenshots();
    let value = json_run(
        &directory,
        &[
            "wide.png",
            "tall.png",
            "--layout",
            "horizontal",
            "--fit-height",
            "min",
            "--max-width",
            "75",
            "--print-spec",
        ],
    );

    assert_eq!(value["spec"]["width"], 75);
    assert_eq!(sizes(&value["spec"]), [(60, 30), (15, 30)]);
}
