use image::{Rgb, RgbImage, RgbaImage};
use serde_json::{json, Value};
use std::process::{Command, Output};
use tempfile::TempDir;

fn write_image(directory: &TempDir, name: &str, width: u32, height: u32, color: [u8; 3]) {
    RgbImage::from_pixel(width, height, Rgb(color))
        .save(directory.path().join(name))
        .unwrap();
}

fn run_cli(directory: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_stitchr"))
        .current_dir(directory.path())
        .args(args)
        .output()
        .unwrap()
}

fn print_spec(directory: &TempDir, inputs: [&str; 3]) -> Value {
    let result = run_cli(
        directory,
        &[
            inputs[0],
            inputs[1],
            inputs[2],
            "--layout",
            "grid",
            "--columns",
            "2",
            "--print-spec",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

fn assert_canvas(
    image: &RgbaImage,
    width: u32,
    height: u32,
    expected_pixel: impl Fn(u32, u32) -> [u8; 4],
) {
    assert_eq!(image.dimensions(), (width, height));
    for y in 0..height {
        for x in 0..width {
            assert_eq!(
                image.get_pixel(x, y).0,
                expected_pixel(x, y),
                "unexpected pixel at ({x}, {y})"
            );
        }
    }
}

#[test]
fn cli_renders_three_image_ragged_grid_with_empty_cell_background() {
    let directory = tempfile::tempdir().unwrap();
    write_image(&directory, "a.png", 4, 2, [255, 0, 0]);
    write_image(&directory, "b.png", 2, 4, [0, 0, 255]);
    write_image(&directory, "c.png", 6, 1, [0, 255, 0]);

    let result = run_cli(
        &directory,
        &[
            "a.png",
            "b.png",
            "c.png",
            "--layout",
            "grid",
            "--columns",
            "2",
            "--output",
            "stitched.png",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let image = image::open(directory.path().join("stitched.png"))
        .unwrap()
        .to_rgba8();
    assert_canvas(&image, 8, 5, |x, y| {
        if x < 4 && y < 2 {
            [255, 0, 0, 255]
        } else if (6..8).contains(&x) && y < 4 {
            [0, 0, 255, 255]
        } else if x < 6 && y == 4 {
            [0, 255, 0, 255]
        } else {
            [0, 0, 0, 255]
        }
    });
}

#[test]
fn three_image_spec_tracks_mixed_png_and_jpeg_dimensions_identically() {
    let png_directory = tempfile::tempdir().unwrap();
    let mixed_directory = tempfile::tempdir().unwrap();
    for directory in [&png_directory, &mixed_directory] {
        write_image(directory, "a.png", 4, 2, [255, 0, 0]);
        write_image(directory, "c.png", 6, 1, [0, 255, 0]);
    }
    write_image(&png_directory, "b.png", 2, 4, [0, 0, 255]);
    write_image(&mixed_directory, "b.jpg", 2, 4, [0, 0, 255]);

    let png_spec = print_spec(&png_directory, ["a.png", "b.png", "c.png"]);
    let mixed_spec = print_spec(&mixed_directory, ["a.png", "b.jpg", "c.png"]);

    assert_eq!(png_spec["width"], 8);
    assert_eq!(png_spec["height"], 5);
    assert_eq!(
        png_spec["images"],
        json!([
            { "name": "a.png", "position": { "x": 0, "y": 0 } },
            { "name": "b.png", "position": { "x": 6, "y": 0 } },
            { "name": "c.png", "position": { "x": 0, "y": 4 } }
        ])
    );
    assert_eq!(mixed_spec["width"], png_spec["width"]);
    assert_eq!(mixed_spec["height"], png_spec["height"]);
    assert_eq!(mixed_spec["layout"], png_spec["layout"]);
    let positions = |spec: &Value| {
        spec["images"]
            .as_array()
            .unwrap()
            .iter()
            .map(|image| image["position"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(positions(&mixed_spec), positions(&png_spec));
    assert!(png_spec.get("sizes").is_none());
    assert!(png_spec.get("max_size").is_none());
}

#[test]
fn cli_renders_split_of_same_size_images() {
    let directory = tempfile::tempdir().unwrap();
    write_image(&directory, "light.png", 10, 5, [255, 0, 0]);
    write_image(&directory, "dark.png", 10, 5, [0, 0, 255]);

    let result = run_cli(
        &directory,
        &[
            "light.png",
            "dark.png",
            "--layout",
            "split",
            "-o",
            "split.png",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let image = image::open(directory.path().join("split.png"))
        .unwrap()
        .to_rgba8();
    let first_blue_column_per_row = [6, 5, 5, 5, 4];
    assert_canvas(&image, 10, 5, |x, y| {
        if x < first_blue_column_per_row[y as usize] {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        }
    });
}
