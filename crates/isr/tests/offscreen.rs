// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use std::{fs, path::PathBuf, process::Command};

#[test]
fn offscreen_document_order_security_and_auto_height() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../out/native-test");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("input.html"), r#"<!doctype html><html><head><style>
      body{margin:0}#safe{width:100px;height:1200px;padding:10px;border:2px solid black;box-sizing:border-box}
      </style></head><body><div id="safe" class="panel"></div>
      <script>document.querySelector('div').id='unsafe';</script>
      <img src="https://example.org/blocked.png"><div style="display:none" id="hidden"></div></body></html>"#).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_isr"))
        .arg("render")
        .arg(dir.join("input.html"))
        .args(["--width", "412", "--height-auto", "--out"])
        .arg(dir.join("out.png"))
        .arg("--layout")
        .arg(dir.join("layout.json"))
        .env("RAYON_NUM_THREADS", "4")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let layout: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("layout.json")).unwrap()).unwrap();
    let elements = layout["elements"].as_array().unwrap();
    assert_eq!(elements[0]["tag"], "html");
    assert_eq!(elements[1]["tag"], "head");
    let safe = elements.iter().find(|e| e["id"] == "safe").unwrap();
    assert_eq!(safe["width"], 100.0);
    assert_eq!(safe["height"], 1200.0);
    assert!(elements.iter().all(|e| e["id"] != "unsafe"));
    assert!(elements.iter().any(|e| e["id"] == "hidden"));
    assert!(layout["height"].as_u64().unwrap() >= 1200);
    assert_eq!(
        layout["blocked_resources"][0],
        "https://example.org/blocked.png"
    );
    let png = fs::read(dir.join("out.png")).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 412);
    assert!(u32::from_be_bytes(png[20..24].try_into().unwrap()) >= 1200);
}
