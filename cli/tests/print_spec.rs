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
    assert_eq!(spec["version"], 1);
    assert_eq!(spec["layout"], "vertical");
    assert_eq!(
        spec["paths"],
        json!(["/missing/first.png", "/missing/second.jpg"])
    );
    assert_eq!(spec["spacing"], 8);
    assert_eq!(spec["max_size"], Value::Null);
}
