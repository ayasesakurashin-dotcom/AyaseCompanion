use ayase_core::{
    package::{self, pack, validate},
    plugin::{Plugin, Runtime},
    schema,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
};
use zip::{write::SimpleFileOptions, ZipWriter};

fn fixture() -> (BTreeMap<String, Vec<u8>>, Value) {
    let im = image::RgbaImage::from_pixel(16, 16, image::Rgba([50, 100, 150, 255]));
    let mut image = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(im)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let model = json!({"schemaVersion":"1.0","logicalSize":{"width":16,"height":16},"anchors":{"feet":{"x":0.5,"y":1},"bubble":{"x":0.5,"y":0}},"clips":{"idle":{"asset":"image","layout":{"type":"single"},"frameDurationMs":1000,"direction":"forward"}},"baseModes":{"idle":{"loop":"idle"}}});
    let files = BTreeMap::from([
        ("idle.png".into(), image.into_inner()),
        ("sprite.json".into(), serde_json::to_vec(&model).unwrap()),
        ("future.bin".into(), vec![0, 255, 7]),
    ]);
    let m = json!({"format":"ayase.character","formatVersion":"1.0","packageId":"test.static","version":"0.1.0","displayName":"静态测试","avatar":{"renderer":"sprite.v1","descriptor":"sprite.json"},"assets":{"image":{"kind":"image","path":"idle.png"}},"pluginPresets":[{"presetId":"future","pluginId":"unknown.future","configVersion":"9.0","enabled":true,"config":{"unrecognized":[{"x":1}],"asset":"future.bin"}}],"extensions":{"future":{"hello":true}}});
    (files, m)
}
fn zip_raw(files: BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    for (p, b) in files {
        z.start_file(p, SimpleFileOptions::default()).unwrap();
        z.write_all(&b).unwrap();
    }
    z.finish().unwrap().into_inner()
}
#[test]
fn static_idle_unknown_plugin_and_export_roundtrip() {
    let (f, m) = fixture();
    let a = validate(&pack(f, m, "manifest.json").unwrap()).unwrap();
    let b = validate(&pack(a.files.clone(), a.manifest.clone(), "manifest.json").unwrap()).unwrap();
    assert_eq!(a.manifest, b.manifest);
    assert_eq!(a.files, b.files);
    assert_eq!(b.files["future.bin"], [0, 255, 7]);
}
#[test]
fn damaged_idle_or_hash_never_activates() {
    let (f, m) = fixture();
    let good = validate(&pack(f, m, "manifest.json").unwrap()).unwrap();
    let mut changed = good.files.clone();
    changed.insert("idle.png".into(), b"corrupt".to_vec());
    assert!(validate(&zip_raw(changed.clone()))
        .unwrap_err()
        .contains("散列"));
    let mut m = good.manifest.clone();
    m.as_object_mut().unwrap().remove("integrity");
    changed.insert("manifest.json".into(), serde_json::to_vec(&m).unwrap());
    assert!(validate(&zip_raw(changed)).unwrap_err().contains("idle"));
}
#[test]
fn incompatible_renderer_fallback_and_major_version() {
    let (f, mut m) = fixture();
    m["formatVersion"] = json!("2.0");
    assert!(validate(&pack(f.clone(), m.clone(), "manifest.json").unwrap()).is_err());
    m["formatVersion"] = json!("1.0");
    m["avatar"]["renderer"] = json!("future.3d");
    assert!(validate(&pack(f.clone(), m.clone(), "manifest.json").unwrap()).is_err());
    m["avatar"]["fallback"] = json!({"renderer":"sprite.v1","descriptor":"sprite.json"});
    assert!(!validate(&pack(f, m, "manifest.json").unwrap())
        .unwrap()
        .report
        .warnings
        .is_empty());
}
#[test]
fn missing_optional_asset_is_warning_but_listed_integrity_is_error() {
    let (f, mut m) = fixture();
    m["assets"]["voice"] = json!({"kind":"audio","path":"absent.wav"});
    let p = validate(&pack(f, m, "manifest.json").unwrap()).unwrap();
    assert!(p.report.warnings.iter().any(|s| s.contains("voice")));
    let mut m = p.manifest.clone();
    m["integrity"]["absent.wav"] = json!("0".repeat(64));
    let mut files = p.files;
    files.insert("manifest.json".into(), serde_json::to_vec(&m).unwrap());
    assert!(validate(&zip_raw(files)).is_err());
}
#[test]
fn traversal_symlink_and_directory_collision_rejected() {
    assert!(package::read_zip(&zip_raw(BTreeMap::from([("../escape".into(), vec![])]))).is_err());
    assert!(package::read_zip(&zip_raw(BTreeMap::from([
        ("a".into(), vec![]),
        ("a/x".into(), vec![])
    ])))
    .is_err());
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    z.add_symlink("link", "outside", SimpleFileOptions::default())
        .unwrap();
    assert!(package::read_zip(&z.finish().unwrap().into_inner()).is_err());
}
#[test]
fn no_private_runtime_identity_in_exports() {
    let (f, mut m) = fixture();
    m["instanceId"] = json!("private");
    assert!(pack(f, m, "manifest.json").is_err());
}
fn nightly() -> Plugin {
    Plugin {
        manifest: serde_json::from_str(include_str!(
            "../../../plugins/examples/nightly-rest/plugin.json"
        ))
        .unwrap(),
        schema: serde_json::from_str(include_str!(
            "../../../plugins/examples/nightly-rest/schemas/config.schema.json"
        ))
        .unwrap(),
        rules: serde_json::from_str(include_str!(
            "../../../plugins/examples/nightly-rest/rules/main.json"
        ))
        .unwrap(),
    }
}
#[test]
fn schedule_is_external_configured_and_deduplicated_per_instance() {
    let p = nightly();
    let a=p.configure(&json!({"presetId":"a","configVersion":"1.0","enabled":true,"config":{"timeZone":"Asia/Tokyo"}}),&json!({}),0);
    let b=p.configure(&json!({"presetId":"b","configVersion":"1.0","enabled":true,"config":{"timeZone":"Asia/Tokyo"}}),&json!({}),0);
    let now = "2026-10-26T14:00:00Z".parse().unwrap();
    let mut runtime = Runtime::default();
    assert_eq!(
        p.dispatch(&a, "clock.tick", None, now, &mut runtime).len(),
        1
    );
    assert_eq!(
        p.dispatch(&a, "clock.tick", None, now, &mut runtime).len(),
        0
    );
    assert_eq!(
        p.dispatch(&b, "clock.tick", None, now, &mut runtime).len(),
        1
    );
}
#[test]
fn schedule_disabled_missed_and_changed_time() {
    let p = nightly();
    let preset = json!({"presetId":"a","configVersion":"1.0","enabled":false,"config":{"time":"22:30","timeZone":"Asia/Tokyo"}});
    let c = p.configure(&preset, &json!({}), 0);
    let mut runtime = Runtime::default();
    assert!(p
        .dispatch(
            &c,
            "clock.tick",
            None,
            "2026-10-26T13:30:00Z".parse().unwrap(),
            &mut runtime
        )
        .is_empty());
    let c = p.configure(&preset, &json!({"enabled":true}), 0);
    assert_eq!(
        p.dispatch(
            &c,
            "clock.tick",
            None,
            "2026-10-26T13:30:00Z".parse().unwrap(),
            &mut runtime
        )
        .len(),
        1
    );
    assert!(p
        .dispatch(
            &c,
            "clock.tick",
            None,
            "2026-10-27T13:40:00Z".parse().unwrap(),
            &mut runtime
        )
        .is_empty());
}
#[test]
fn invalid_time_weight_and_code_action_are_rejected() {
    let mut p = nightly();
    let mut config = p.manifest["defaults"].clone();
    config["time"] = json!("24:00");
    assert!(p.validate_config(&config).is_err());
    config["time"] = json!("23:00");
    config["response"]["variants"][0]["weight"] = json!(-1);
    assert!(p.validate_config(&config).is_err());
    p.rules["rules"][0]["actions"][0]["type"] = json!("system.exec");
    assert!(schema::validate(&schema::contract("rules"), &p.rules).is_err());
}
#[test]
fn complete_export_does_not_restore_default_reply_rows() {
    let p = nightly();
    let preset = json!({"presetId":"a","configVersion":"1.0","configMode":"complete","enabled":true,"config":{"time":"22:00","timeZone":"system","missedPolicy":"skip","ttlSeconds":120,"response":{"mode":"fixed","variants":[{"id":"custom","text":"定制台词","audioAsset":null}]}}});
    let c = p.configure(&preset, &json!({}), 0);
    assert!(c.enabled);
    assert_eq!(
        c.config["response"]["variants"].as_array().unwrap().len(),
        1
    );
    assert_eq!(c.config["response"]["variants"][0]["id"], "custom");
}
