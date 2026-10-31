use super::super::tests::{archive, TestRoot};
use super::*;

fn fixture() -> (TestRoot, Host) {
    let root = TestRoot::new();
    let mut h = Host::open(root.0.clone()).unwrap();
    let package = h.stage_archive(&archive("0.3.0")).unwrap();
    activate(&mut h, &package.hash, "new").unwrap();
    (root, h)
}
fn start(h: &mut Host) -> (u64, String) {
    h.scheduler.tick(h.now());
    h.prepare_variant_start();
    let task = h.scheduler.active().unwrap().clone();
    assert!(h.start_presentation(task.id, task.generation, h.now()));
    (
        task.id,
        task.plan["fixedChoice"]["id"].as_str().unwrap().into(),
    )
}
fn finish(h: &mut Host, id: u64) {
    h.scheduler.finish(id, Status::Completed, "test", h.now());
}

#[test]
fn material_channels_keep_order_repeats_and_one_task_boundary() {
    let (_root, mut h) = fixture();
    let p = h.package.as_mut().unwrap();
    p.manifest["assets"]["one"] = json!({"kind":"audio","path":"one.wav"});
    p.manifest["assets"]["two"] = json!({"kind":"audio","path":"two.wav"});
    p.manifest["assets"]["t1"] = json!({"kind":"text","path":"one.txt"});
    p.manifest["assets"]["t2"] = json!({"kind":"text","path":"two.txt"});
    p.files.insert("one.wav".into(), vec![1]);
    p.files.insert("two.wav".into(), vec![2]);
    p.files.insert("one.txt".into(), b"A, keep\nline".to_vec());
    p.files.insert("two.txt".into(), b"B".to_vec());
    p.model.as_mut().unwrap()["semanticCues"] =
        json!({"first":{"clip":"idle","repeat":2},"second":{"clip":"idle"}});
    let v = json!({"id":"seq","textAsset":["t1","t2"],"audioAsset":"one,two\none","cue":["first","second"],"readMs":1000});
    let (plan, channels) = h.resolve_variant(&v, "plugin").unwrap();
    assert_eq!(channels, vec!["text", "audio", "animation"]);
    assert_eq!(plan["textSegments"], json!(["A, keep\nline", "B"]));
    assert_eq!(plan["text"], "A, keep\nline\nB");
    assert_eq!(plan["clips"], json!(["idle", "idle", "idle"]));
    let parts = plan["audioSegments"].as_array().unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], parts[2]);
    assert_ne!(parts[0], parts[1]);
    let id = h
        .present(v, "plugin", Priority::Background, true, 60_000, 0)
        .unwrap();
    h.scheduler.tick(h.now());
    let generation = h.scheduler.active().unwrap().generation;
    assert!(h.start_presentation(id, generation, h.now()));
    let queued = h
        .present(
            json!({"id":"next","text":"next"}),
            "host.user",
            Priority::User,
            false,
            60_000,
            0,
        )
        .unwrap();
    for channel in ["animation", "audio"] {
        assert!(h.scheduler.channel(id, generation, channel, false, h.now()));
        h.scheduler.tick(h.now());
        assert_eq!(h.scheduler.active().unwrap().id, id);
    }
    assert!(h.scheduler.channel(id, generation, "text", false, h.now()));
    h.scheduler.tick(h.now());
    assert_eq!(h.scheduler.active().unwrap().id, queued);
    h.store.muted = true;
    let (muted, channels) = h
        .resolve_variant(
            &json!({"id":"muted","text":["first","second"],"audioAsset":["one","two"]}),
            "plugin",
        )
        .unwrap();
    assert_eq!(muted["textSegments"], json!(["first", "second"]));
    assert_eq!(channels, vec!["text"]);
}

#[test]
fn no_repeat_uses_actual_start_order_and_survives_restart_per_instance() {
    let (root, mut h) = fixture();
    let response = json!({"mode":"fixed","variants":[{"id":"a","text":"A","weight":10000},{"id":"b","text":"B","weight":0.001}]});
    // All are prepared before any actual start, then a later higher priority goes first.
    let first = h
        .present_variants(
            response.clone(),
            "plugin",
            Priority::Background,
            true,
            60_000,
            0,
        )
        .unwrap();
    let high = h
        .present_variants(
            response.clone(),
            "plugin",
            Priority::Important,
            true,
            60_000,
            0,
        )
        .unwrap();
    let (id, a) = start(&mut h);
    assert_eq!(id, high);
    finish(&mut h, id);
    let (id, b) = start(&mut h);
    assert_eq!(id, first);
    assert_ne!(a, b);
    finish(&mut h, id);
    for _ in 0..8 {
        h.present_variants(
            response.clone(),
            "plugin",
            Priority::Normal,
            true,
            60_000,
            0,
        )
        .unwrap();
        let (id, c) = start(&mut h);
        assert_ne!(b, c);
        finish(&mut h, id);
        h.present_variants(
            response.clone(),
            "plugin",
            Priority::Normal,
            true,
            60_000,
            0,
        )
        .unwrap();
        let (id, c) = start(&mut h);
        assert_eq!(b, c);
        finish(&mut h, id);
    }
    let last = b;
    let owner = h.instance().unwrap().id.clone();
    let hash = h.instance().unwrap().package_hash.clone();
    drop(h);
    let mut h = Host::open(root.0.clone()).unwrap();
    h.present_variants(
        response.clone(),
        "plugin",
        Priority::Normal,
        true,
        60_000,
        0,
    )
    .unwrap();
    let (id, c) = start(&mut h);
    assert_ne!(last, c);
    finish(&mut h, id);
    activate(&mut h, &hash, "new").unwrap();
    assert_ne!(h.instance().unwrap().id, owner);
    let (plan, _) = h.variant_plan(&response, "plugin", "fixed", 0.0).unwrap();
    assert_eq!(plan["fixedChoice"]["id"], "a");
}

#[test]
fn invalid_cancelled_and_duplicate_effects_do_not_block_choice() {
    let (_root, mut h) = fixture();
    let response = json!({"mode":"fixed","variants":[{"id":"a","text":"same"},{"id":"clone","text":"same"},{"id":"b","text":"different"},{"id":"missing","audioAsset":"absent"}]});
    let cancelled = h
        .present_variants(
            response.clone(),
            "plugin",
            Priority::Normal,
            true,
            60_000,
            0,
        )
        .unwrap();
    h.scheduler
        .finish(cancelled, Status::Cancelled, "test", h.now());
    assert!(!h
        .instance()
        .unwrap()
        .runtime
        .state
        .contains_key("variant:plugin:fixed"));
    h.present_variants(
        response.clone(),
        "plugin",
        Priority::Normal,
        true,
        60_000,
        0,
    )
    .unwrap();
    let (id, _) = start(&mut h);
    let last = h.scheduler.active().unwrap().plan["text"].clone();
    finish(&mut h, id);
    for unit in [0.0, 0.5, 0.99] {
        let (plan, _) = h.variant_plan(&response, "plugin", "fixed", unit).unwrap();
        assert_ne!(plan["text"], last);
        assert_ne!(plan["fixedChoice"]["id"], "missing");
    }
    let single = json!({"mode":"fixed","variants":[{"id":"a","text":"same"}]});
    assert!(h.variant_plan(&single, "plugin", "fixed", 0.0).is_ok());
    // Fixed and explicit fallback keep separate history, even with identical IDs.
    assert!(h.variant_plan(&response, "plugin", "fallback", 0.0).is_ok());
}
