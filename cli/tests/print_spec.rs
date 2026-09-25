use serde_json::{json, Value};
use std::process::Command;
use stitchr_core::{export_image, DynamicImage, ExportFormat};
use tempfile::TempDir;

fn write_image(directory: &TempDir, name: &str, width: u32, height: u32, format: ExportFormat) {
    export_image(
        &DynamicImage::new_rgb8(width, height),
        directory.path().join(name),
        format,
    )
    .unwrap();
}

#[test]
fn prints_resolved_spec_with_loaded_image_metadata() {
    let directory = tempfile::tempdir().unwrap();
    write_image(&directory, "first.png", 2, 1, ExportFormat::Png);
    write_image(&directory, "second.jpg", 1, 1, ExportFormat::Jpeg);
    let first = directory.path().join("first.png");
    let second = directory.path().join("second.jpg");

    let result = Command::new(env!("CARGO_BIN_EXE_stitchr"))
        .arg(first.to_str().unwrap())
        .arg(second.to_str().unwrap())
        .args(["--layout", "vertical", "--spacing", "8", "--print-spec"])
        .output()
        .unwrap();

    assert!(result.status.success());
    let spec: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(spec.get("version").is_none());
    assert_eq!(spec["layout"]["type"], "stack");
    assert_eq!(spec["layout"]["direction"], "vertical");
    assert_eq!(spec["layout"]["spacing"], 8);
    assert_eq!(spec["layout"]["alignment"], "start");
    assert_eq!(
        spec["images"],
        json!([
            { "name": "first.png", "position": { "x": 8, "y": 8 } },
            { "name": "second.jpg", "position": { "x": 8, "y": 17 } }
        ])
    );
    assert_eq!(spec["width"], 18);
    assert_eq!(spec["height"], 26);
    assert!(spec.get("paths").is_none());
    assert!(spec.get("output").is_none());
    assert!(spec.get("spacing").is_none());
    assert!(spec.get("sizes").is_none());
    assert!(spec.get("max_size").is_none());
    assert!(spec["images"][0].get("width").is_none());
    assert_eq!(
        spec["background_color"],
        json!({ "red": 0, "green": 0, "blue": 0 })
    );
}

#[test]
fn freeform_print_spec_omits_spacing_and_alignment() {
    let directory = tempfile::tempdir().unwrap();
    write_image(&directory, "first.png", 2, 1, ExportFormat::Png);
    write_image(&directory, "second.png", 1, 1, ExportFormat::Png);
    let first = directory.path().join("first.png");
    let second = directory.path().join("second.png");

    let result = Command::new(env!("CARGO_BIN_EXE_stitchr"))
        .arg(first.to_str().unwrap())
        .arg(second.to_str().unwrap())
        .args([
            "--layout",
            "freeform",
            "--position",
            "1",
            "2",
            "--position",
            "3",
            "4",
            "--spacing",
            "20",
            "--alignment",
            "end",
            "--print-spec",
        ])
        .output()
        .unwrap();

    assert!(result.status.success());
    let spec: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        spec["images"],
        json!([
            { "name": "first.png", "position": { "x": 1, "y": 2 } },
            { "name": "second.png", "position": { "x": 3, "y": 4 } }
        ])
    );
    assert_eq!(spec["layout"]["type"], "freeform");
    assert_eq!(
        spec["layout"]["positions"],
        json!([{ "x": 1, "y": 2 }, { "x": 3, "y": 4 }])
    );
    assert!(spec["layout"].get("spacing").is_none());
    assert!(spec["layout"].get("alignment").is_none());
    assert!(spec["layout"].get("columns").is_none());
    assert!(spec.get("sizes").is_none());
    assert!(spec.get("max_size").is_none());
    assert_eq!(spec["width"], 4);
    assert_eq!(spec["height"], 5);
    assert!(spec.get("paths").is_none());
    assert!(spec.get("output").is_none());
}
