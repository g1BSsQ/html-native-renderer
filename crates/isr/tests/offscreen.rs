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

#[test]
fn offscreen_table_part_geometry_does_not_resolve_unstyled_nodes() {
    // Exercise the exact render + JSON export path used by the CLI, including
    // head metadata, a style element and a display:none subtree. Geometry
    // export must not initiate style resolution after the layout phase.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../out/native-test-table-part-export");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("input.html"), r#"<!doctype html>
        <html><head><meta charset="utf-8"><title>Table</title>
        <style>body{margin:0} .cell{display:table-cell;width:60px;height:28px;
            background:#ff8000}</style></head><body>
        <div id="hidden" style="display:none"><div id="unlaid"
            style="display:table-row-group"><div style="display:table-row">
            <div class="cell"></div></div></div></div>
        <div id="table" style="display:table;width:120px;border-collapse:collapse">
            <div id="group" style="display:table-row-group">
                <div id="row" style="display:table-row">
                    <div id="left" class="cell"></div>
                    <div id="right" class="cell"></div>
                </div>
            </div>
        </div>
        </body></html>"#).unwrap();

    let result = Command::new(env!("CARGO_BIN_EXE_isr"))
        .arg("render")
        .arg(dir.join("input.html"))
        .args(["--width", "320", "--height-auto", "--out"])
        .arg(dir.join("out.png"))
        .arg("--layout")
        .arg(dir.join("layout.json"))
        .env("RAYON_NUM_THREADS", "4")
        .output()
        .unwrap();
    assert!(result.status.success(), "{}",
        String::from_utf8_lossy(&result.stderr));

    let layout: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("layout.json")).unwrap()).unwrap();
    let elements = layout["elements"].as_array().unwrap();
    assert!(elements.iter().any(|e| e["tag"] == "meta"));
    assert!(elements.iter().any(|e| e["tag"] == "style"));
    let part = |id: &str| elements.iter().find(|e| e["id"] == id).unwrap();
    for id in ["table", "group", "row", "left", "right"] {
        assert!(part(id)["width"].as_f64().unwrap() > 20.0,
            "missing table geometry: {id}");
        assert!(part(id)["height"].as_f64().unwrap() > 20.0,
            "missing table height: {id}");
    }
    assert_eq!(part("unlaid")["width"], 0.0);
    assert_eq!(part("unlaid")["height"], 0.0);
    assert!(layout["height"].as_u64().unwrap() >= 28);
    let png = fs::read(dir.join("out.png")).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(png.len() > 100, "rendered PNG is empty");
}
