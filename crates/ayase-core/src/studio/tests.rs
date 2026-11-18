use super::*;
#[test]
fn creator_checks_actual_image_without_mutating_incomplete_project() {
    let mut p = Project::character();
    p.asset("picture", "image", "picture.png", png()).unwrap();
    p.asset("text", "text", "text.txt", b"hello".to_vec())
        .unwrap();
    let original = serde_json::to_value(&p.document).unwrap();
    let single = json!({"asset":"picture","layout":{"type":"single"},"frameDurationMs":1000,"direction":"forward"});
    assert_eq!(
        p.check_clip(&single).unwrap(),
        json!({"valid":true,"width":32,"height":32})
    );
    let mut grid = json!({"asset":"picture","layout":{"type":"grid","columns":3,"rows":3,"cellWidth":10,"cellHeight":10,"frameCount":8},"frameDurationMs":200,"direction":"ping-pong","returnToStart":true,"startReverse":false});
    // Unused image edges and unused final cells are valid author choices.
    assert!(p.check_clip(&grid).is_ok());
    grid["layout"]["frameCount"] = json!(10);
    assert!(p.check_clip(&grid).is_err());
    grid["layout"]["frameCount"] = json!(8);
    grid["layout"]["cellWidth"] = json!(11);
    assert!(p.check_clip(&grid).is_err());
    grid["layout"] = json!({"type":"rects","frames":[{"x":0,"y":0,"width":32,"height":16},{"x":0,"y":0,"width":16,"height":32}]});
    assert!(p.check_clip(&grid).is_ok());
    grid["layout"]["frames"][1]["x"] = json!(17);
    assert!(p.check_clip(&grid).is_err());
    for (key, value) in [
        ("asset", json!("missing")),
        ("asset", json!("text")),
        ("direction", json!("other")),
        ("frameDurationMs", json!(0)),
    ] {
        let mut bad = single.clone();
        bad[key] = value;
        assert!(p.check_clip(&bad).is_err());
    }
    assert_eq!(serde_json::to_value(&p.document).unwrap(), original);
}
#[test]
fn decorations_roundtrip_references_and_validation() {
    let mut p = Project::character();
    p.asset("body", "image", "assets/body.png", png()).unwrap();
    p.asset("ornament", "image", "assets/ornament.png", png())
        .unwrap();
    p.document.model["clips"]["idle"] = json!({"asset":"body","layout":{"type":"single"},"frameDurationMs":1000,"direction":"forward"});
    p.document.model["clips"]["ornament"] = json!({"asset":"ornament","layout":{"type":"single"},"frameDurationMs":200,"direction":"forward"});
    p.document.model["decorationVersion"] = json!("1.0");
    let layer = json!({"id":"wreath","side":"foreground","order":0,"enabled":true,"source":{"type":"clip","clip":"ornament"},"anchor":{"x":0.5,"y":1},"pivot":{"x":0.5,"y":1},"offset":{"x":0,"y":0},"width":null,"height":null,"keepAspect":true,"scale":1,"speed":1,"states":["music"]});
    p.document.model["decorations"] = json!([layer.clone()]);
    let mut next = p.document.clone();
    next.model["clips"]
        .as_object_mut()
        .unwrap()
        .remove("ornament");
    assert!(p
        .save_document(p.document.revision, next)
        .unwrap_err()
        .contains("仍被装饰引用"));
    assert!(p.delete_asset("ornament").is_err());
    p.rename_asset("ornament", "ornament-new").unwrap();
    // Asset renaming must not rename a different namespace's clip ID.
    assert_eq!(
        p.document.model["decorations"][0]["source"]["clip"],
        "ornament"
    );
    assert_eq!(
        p.document.model["clips"]["ornament"]["asset"],
        "ornament-new"
    );
    let package = p.validated().unwrap();
    assert!(package.report.warnings.is_empty());
    let restored = Project::from_package(package).unwrap();
    assert_eq!(
        restored.document.model["decorations"],
        p.document.model["decorations"]
    );
    for (key, value) in [("speed", json!(0)), ("scale", json!(-1))] {
        let mut invalid = restored.clone();
        invalid.document.model["decorations"][0][key] = value;
        assert!(invalid.export().is_err());
    }
    let mut invalid = restored.clone();
    invalid.document.model["decorations"]
        .as_array_mut()
        .unwrap()
        .push(layer);
    assert!(invalid.export().is_err());
}
fn png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(32, 32, image::Rgba([20, 100, 180, 255]));
    let mut b = std::io::Cursor::new(vec![]);
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut b, image::ImageFormat::Png)
        .unwrap();
    b.into_inner()
}
#[test]
fn create_static_roundtrip_preserves_unknowns_and_cannot_delete_idle() {
    let mut p = Project::character();
    p.asset("visual.idle", "image", "assets/idle.png", png())
        .unwrap();
    p.document.model["clips"]["idle"] = json!({"asset":"visual.idle","layout":{"type":"single"},"frameDurationMs":1000,"direction":"forward"});
    p.document.manifest["extensions"] = json!({"future":{"opaque":[1,2]}});
    assert!(p.delete_asset("visual.idle").is_err());
    p.rename_asset("visual.idle", "visual.main").unwrap();
    assert_eq!(p.document.model["clips"]["idle"]["asset"], "visual.main");
    let data = p.export().unwrap();
    let imported = Project::from_package(package::validate(&data).unwrap()).unwrap();
    let next = imported.validated().unwrap();
    assert_eq!(
        next.manifest["extensions"],
        p.document.manifest["extensions"]
    );
    assert_eq!(next.files["assets/idle.png"], png());
}
#[test]
fn plugin_and_theme_export_use_shared_validation() {
    let p = Project::rule_plugin();
    let v = p.validated().unwrap();
    assert_eq!(v.report.kind, "plugin");
    let mut t = Project::theme();
    assert_eq!(t.validated().unwrap().report.kind, "theme");
    t.document.manifest["maxWidth"] = json!(800);
    t.document.manifest["inputOpacity"] = json!(35);
    assert!(t.export().is_ok());
    t.document.manifest["offsetX"] = json!(99999);
    assert!(t.export().is_err());
}
#[test]
fn revisions_private_fields_and_unknown_plugin_references_are_protected() {
    let mut p = Project::character();
    let mut next = p.document.clone();
    next.manifest["instanceId"] = json!("never export");
    assert!(p.save_document(0, next).is_err());
    let next = p.document.clone();
    p.save_document(0, next.clone()).unwrap();
    assert!(p.save_document(0, next).is_err());
    p.asset("voice.one", "audio", "assets/one.wav", vec![0; 10])
        .unwrap();
    p.document.manifest["pluginPresets"] =
        json!([{"presetId":"unknown","pluginId":"future.unknown","config":{"opaque":"voice.one"}}]);
    assert!(p.rename_asset("voice.one", "voice.new").is_err());
    assert!(p.delete_asset("voice.one").is_err());
}

#[test]
fn nine_slice_theme_is_data_only_integrity_checked_and_unknown_plugin_roundtrips() {
    let mut p = Project::theme();
    p.asset("panel", "image", "panel.png", png()).unwrap();
    p.document.manifest["backgroundAsset"] = json!("panel");
    p.document.manifest["sliceLeft"] = json!(8);
    p.document.manifest["sliceRight"] = json!(8);
    assert!(p.validated().is_ok());
    p.document.manifest["sliceRight"] = json!(30);
    assert!(p.export().is_err());
    p.document.manifest["sliceRight"] = json!(8);
    p.document.manifest["css"] = json!("body{display:none}");
    assert!(p.export().is_err());
    p.document.manifest.as_object_mut().unwrap().remove("css");
    p.files.remove("panel.png");
    assert!(p.export().is_err());
    let mut p = Project::character();
    p.asset("idle", "image", "idle.png", png()).unwrap();
    p.document.model["clips"]["idle"] = json!({"asset":"idle","layout":{"type":"single"},"frameDurationMs":1000,"direction":"forward"});
    p.document.manifest["pluginPresets"] = json!([{"presetId":"future","pluginId":"future.plugin","configVersion":"2.0","enabled":true,"priority":"normal","config":{"opaque":[null,1,{"v":"idle"}]}}]);
    let expected = p.document.manifest["pluginPresets"].clone();
    assert_eq!(
        effective_presets(&p.document.manifest, &plugin::builtin()).unwrap(),
        expected
    );
    let imported = Project::from_package(p.validated().unwrap()).unwrap();
    assert_eq!(imported.document.manifest["pluginPresets"], expected);
}
