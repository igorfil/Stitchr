use serde_json::Value;
use std::{fs, path::Path, process::Command};

#[test]
fn cookbook_commands_match_documented_outputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("docs/examples/cases.json")).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["working_directory"], "repository_root");
    let cases = manifest["cases"].as_array().unwrap();
    assert!(!cases.is_empty());

    for case in cases {
        let id = case["id"].as_str().unwrap();
        let directory = tempfile::tempdir().unwrap();
        for input in manifest["inputs"].as_array().unwrap() {
            let path = input.as_str().unwrap();
            let target = directory.path().join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(root.join(path), target).unwrap();
        }
        let args = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|argument| argument.as_str().unwrap())
            .collect::<Vec<_>>();
        let expected = &case["expected"];
        let output_path = directory.path().join(expected["output"].as_str().unwrap());
        let preview = Command::new(env!("CARGO_BIN_EXE_stitchr"))
            .current_dir(directory.path())
            .args(&args)
            .arg("--print-spec")
            .output()
            .unwrap();
        assert!(
            preview.status.success(),
            "{id}: {}",
            String::from_utf8_lossy(&preview.stdout)
        );
        assert!(preview.stderr.is_empty(), "{id}");
        let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
        assert_eq!(preview["spec"]["width"], expected["width"], "{id}");
        assert_eq!(preview["spec"]["height"], expected["height"], "{id}");
        assert!(!output_path.exists(), "{id}: --print-spec wrote an image");

        let result = Command::new(env!("CARGO_BIN_EXE_stitchr"))
            .current_dir(directory.path())
            .args(&args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{id}: {}",
            String::from_utf8_lossy(&result.stdout)
        );
        assert!(result.stderr.is_empty(), "{id}");
        let response: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(response["schema_version"], 1, "{id}");
        assert_eq!(response["ok"], true, "{id}");
        for field in ["output", "width", "height", "format"] {
            assert_eq!(response[field], expected[field], "{id}: {field}");
        }
        let bytes = fs::read(output_path).unwrap();
        let format = match expected["format"].as_str().unwrap() {
            "png" => image::ImageFormat::Png,
            "jpeg" => image::ImageFormat::Jpeg,
            other => panic!("{id}: unsupported fixture format {other}"),
        };
        assert_eq!(image::guess_format(&bytes).unwrap(), format, "{id}");
        let actual = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(
            u64::from(actual.width()),
            expected["width"].as_u64().unwrap(),
            "{id}"
        );
        assert_eq!(
            u64::from(actual.height()),
            expected["height"].as_u64().unwrap(),
            "{id}"
        );
        assert!(
            actual.pixels().all(|pixel| pixel[3] == 255),
            "{id}: output must be opaque"
        );

        for sample in expected["pixels"].as_array().unwrap() {
            let x = u32::try_from(sample["x"].as_u64().unwrap()).unwrap();
            let y = u32::try_from(sample["y"].as_u64().unwrap()).unwrap();
            let pixel = actual.get_pixel(x, y);
            let tolerance = sample["tolerance"].as_u64().unwrap();
            for (channel, value) in sample["rgba"].as_array().unwrap().iter().enumerate() {
                assert!(
                    u64::from(pixel[channel]).abs_diff(value.as_u64().unwrap()) <= tolerance,
                    "{id}: pixel ({x}, {y}) is {pixel:?}, expected {}",
                    sample["rgba"]
                );
            }
        }

        let reference = image::open(root.join(expected["reference"].as_str().unwrap()))
            .unwrap()
            .to_rgba8();
        assert_eq!(actual.dimensions(), reference.dimensions(), "{id}");
        let tolerance = expected["reference_tolerance"].as_u64().unwrap();
        let mismatch = actual
            .as_raw()
            .iter()
            .zip(reference.as_raw())
            .position(|(actual, reference)| u64::from(actual.abs_diff(*reference)) > tolerance);
        assert!(
            mismatch.is_none(),
            "{id}: reference image mismatch at channel {mismatch:?}"
        );
    }
}
