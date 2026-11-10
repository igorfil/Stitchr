use image::{Rgb, RgbImage};
use serde_json::Value;
use std::process::{Command, Output};
use tempfile::TempDir;

fn fixtures() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    RgbImage::from_pixel(80, 30, Rgb([255, 0, 0]))
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

fn json_run(directory: &TempDir, args: &[&str]) -> Value {
    let mut all = vec!["--json"];
    all.extend_from_slice(args);
    serde_json::from_slice(&run(directory, &all).stdout).unwrap()
}

fn bundled_font() -> String {
    format!(
        "{}/../core/fonts/Inter-SemiBold.ttf",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn outside_labels_move_the_image_below_a_header() {
    let directory = fixtures();
    let inside = json_run(&directory, &["a.png", "--label", "A", "--print-spec"]);
    let outside = json_run(
        &directory,
        &[
            "a.png",
            "--label",
            "A",
            "--label-position",
            "outside",
            "--print-spec",
        ],
    );

    assert_eq!(inside["spec"]["images"][0]["position"]["y"], 2);
    assert!(
        outside["spec"]["images"][0]["position"]["y"]
            .as_u64()
            .unwrap()
            > 2
    );
    assert_eq!(outside["spec"]["frame"]["label_position"], "outside");
    assert!(inside["spec"]["frame"].get("label_position").is_none());
}

#[test]
fn outside_labels_render_without_covering_the_image() {
    let directory = fixtures();
    let value = json_run(
        &directory,
        &[
            "a.png",
            "--label",
            "Label",
            "--label-position",
            "outside",
            "-o",
            "out.png",
        ],
    );
    let image = image::open(directory.path().join("out.png"))
        .unwrap()
        .to_rgba8();
    let top = value["height"].as_u64().unwrap() as u32 - 32;

    assert_eq!(image.get_pixel(2, top).0, [255, 0, 0, 255]);
    assert_eq!(
        image
            .get_pixel(81, value["height"].as_u64().unwrap() as u32 - 3)
            .0,
        [255, 0, 0, 255]
    );
}

#[test]
fn label_color_is_recorded_and_drawn() {
    let directory = fixtures();
    let spec = json_run(
        &directory,
        &[
            "a.png",
            "--label",
            "W",
            "--label-size",
            "30",
            "--label-color",
            "yellow",
            "--print-spec",
        ],
    );
    assert_eq!(
        spec["spec"]["frame"]["label_color"],
        serde_json::json!({ "red": 255, "green": 255, "blue": 0 })
    );

    json_run(
        &directory,
        &[
            "a.png",
            "--label",
            "W",
            "--label-size",
            "30",
            "--label-color",
            "yellow",
            "-o",
            "out.png",
        ],
    );
    let image = image::open(directory.path().join("out.png"))
        .unwrap()
        .to_rgba8();
    assert!(image.pixels().any(|pixel| pixel.0 == [255, 255, 0, 255]));
}

#[test]
fn invalid_label_color_is_an_argument_error() {
    let directory = fixtures();
    let value = json_run(
        &directory,
        &["a.png", "--label", "A", "--label-color", "nope"],
    );

    assert_eq!(value["error"]["code"], "invalid_arguments");
}

#[test]
fn font_path_is_recorded_as_an_absolute_path() {
    let directory = fixtures();
    std::fs::copy(bundled_font(), directory.path().join("font.ttf")).unwrap();
    let value = json_run(
        &directory,
        &[
            "a.png",
            "--label",
            "A",
            "--font",
            "font.ttf",
            "--print-spec",
        ],
    );
    let font = value["spec"]["frame"]["font"].as_str().unwrap();

    assert!(std::path::Path::new(font).is_absolute());
    assert!(font.ends_with("font.ttf"));
}

#[test]
fn custom_font_renders() {
    let directory = fixtures();
    let font = bundled_font();
    let value = json_run(
        &directory,
        &[
            "a.png", "--label", "Font", "--font", &font, "-o", "font.png",
        ],
    );
    json_run(
        &directory,
        &["a.png", "--label", "Font", "-o", "default.png"],
    );

    assert_eq!(value["ok"], true);
    let read = |name: &str| image::open(directory.path().join(name)).unwrap().to_rgba8();
    assert_eq!(read("font.png"), read("default.png"));
}

#[test]
fn missing_font_is_an_input_error_with_details() {
    let directory = fixtures();
    let value = json_run(
        &directory,
        &[
            "a.png",
            "--label",
            "A",
            "--font",
            "missing.ttf",
            "--print-spec",
        ],
    );

    assert_eq!(value["error"]["code"], "input_error");
    assert_eq!(value["error"]["details"]["kind"], "font");
    assert_eq!(value["error"]["details"]["path"], "missing.ttf");
}

#[test]
fn file_that_is_not_a_font_is_an_input_error() {
    let directory = fixtures();
    std::fs::write(directory.path().join("bad.ttf"), "not a font").unwrap();
    let value = json_run(
        &directory,
        &["a.png", "--label", "A", "--font", "bad.ttf", "--print-spec"],
    );

    assert_eq!(value["error"]["code"], "input_error");
    assert_eq!(value["error"]["details"]["kind"], "font");
}

#[test]
fn label_style_without_labels_is_reported() {
    let directory = fixtures();
    let value = json_run(
        &directory,
        &[
            "a.png",
            "--label-position",
            "outside",
            "--label-color",
            "red",
            "--print-spec",
        ],
    );
    let options = value["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|warning| warning["option"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();

    assert_eq!(options, ["--label-position", "--label-color"]);
}

#[test]
fn styled_labels_round_trip_through_a_spec_file() {
    let directory = fixtures();
    let font = bundled_font();
    let args = [
        "a.png",
        "--label",
        "Round trip",
        "--label-position",
        "outside",
        "--label-color",
        "blue",
        "--font",
        &font,
    ];
    let mut print = args.to_vec();
    print.push("--print-spec");
    let spec = json_run(&directory, &print)["spec"].clone();
    std::fs::write(directory.path().join("spec.json"), spec.to_string()).unwrap();
    let mut direct = args.to_vec();
    direct.extend(["-o", "direct.png"]);
    json_run(&directory, &direct);

    let value = json_run(&directory, &["--spec", "spec.json", "-o", "from-spec.png"]);

    assert_eq!(value["ok"], true);
    let read = |name: &str| image::open(directory.path().join(name)).unwrap().to_rgba8();
    assert_eq!(read("from-spec.png"), read("direct.png"));
}

#[test]
fn spec_with_a_missing_font_is_an_input_error() {
    let directory = fixtures();
    let mut spec = json_run(&directory, &["a.png", "--label", "A", "--print-spec"])["spec"].clone();
    spec["frame"]["font"] = Value::from("/missing/font.ttf");
    std::fs::write(directory.path().join("spec.json"), spec.to_string()).unwrap();

    let value = json_run(&directory, &["--spec", "spec.json", "-o", "out.png"]);

    assert_eq!(value["error"]["code"], "input_error");
    assert_eq!(value["error"]["details"]["kind"], "font");
}
