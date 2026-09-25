use serde_json::{json, Value};
use std::process::Command;

#[test]
fn prints_spec_for_nonexistent_inputs_without_rendering() {
    let result = Command::new(env!("CARGO_BIN_EXE_stitchr"))
        .args([
            "/missing/first.png",
            "/missing/second.jpg",
            "--layout",
            "vertical",
            "--spacing",
            "8",
            "--print-spec",
        ])
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
        spec["paths"],
        json!(["/missing/first.png", "/missing/second.jpg"])
    );
    assert!(spec.get("spacing").is_none());
    assert!(spec.get("max_size").is_none());
    assert_eq!(
        spec["background_color"],
        json!({ "red": 0, "green": 0, "blue": 0 })
    );
}
