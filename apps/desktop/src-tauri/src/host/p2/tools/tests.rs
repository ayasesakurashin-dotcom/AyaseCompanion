use super::*;
use crate::host::tests as baseline;
fn fixture() -> (baseline::TestRoot, Host, String, String) {
    let root = baseline::TestRoot::new();
    let mut h = Host::open(root.0.clone()).unwrap();
    let p = h.stage_archive(&baseline::archive("0.1.0")).unwrap();
    activate(&mut h, &p.hash, "new").unwrap();
    h.renderer_ready = true;
    let owner = h.instance().unwrap().id.clone();
    (root, h, owner, p.hash)
}
#[test]
fn previews_wait_for_existing_reply_and_stop_does_not_cancel_chat() {
    let (_root, mut h, owner, _) = fixture();
    let reply = h
        .present(
            json!({"text":"existing reply"}),
            "host.user",
            Priority::User,
            false,
            60000,
            0,
        )
        .unwrap();
    h.tick();
    let (id, generation, dir) = prepare_preview(&mut h, &owner, 60).unwrap();
    complete_preview(&mut h, &owner, id, generation, &dir, Ok(vec![1, 2, 3])).unwrap();
    h.tick();
    assert_eq!(h.scheduler.active().unwrap().id, reply);
    assert_eq!(
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .status,
        Status::Ready
    );
    stop_previews(&mut h, &owner);
    assert_eq!(h.scheduler.active().unwrap().id, reply);
    assert_eq!(
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .status,
        Status::Cancelled
    );
    assert!(h.ai.memory.history(&owner).unwrap()["messages"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[test]
fn stopped_or_switched_preview_cannot_play_late_audio() {
    let (_root, mut h, owner, hash) = fixture();
    let (id, generation, dir) = prepare_preview(&mut h, &owner, 60).unwrap();
    h.cancel_generations();
    assert_eq!(
        complete_preview(&mut h, &owner, id, generation, &dir, Ok(vec![1])).unwrap()["cancelled"],
        true
    );
    assert!(!dir.exists());
    let (id, generation, dir) = prepare_preview(&mut h, &owner, 60).unwrap();
    activate(&mut h, &hash, "new").unwrap();
    assert_eq!(
        complete_preview(&mut h, &owner, id, generation, &dir, Ok(vec![1])).unwrap()["cancelled"],
        true
    );
    assert!(!dir.exists());
    assert!(prepare_preview(&mut h, &owner, 60).is_err());
}
#[test]
fn voice_receipt_remains_with_original_instance_after_switch_and_restart() {
    let (root, mut h, a, hash) = fixture();
    activate(&mut h, &hash, "new").unwrap();
    let b = h.instance().unwrap().id.clone();
    store_voice(
        &mut h,
        VoiceRecord {
            instance_id: a.clone(),
            endpoint: "https://api.fish.audio/v1".into(),
            id: "fixture_voice_1".into(),
            title: "Created for A".into(),
            state: "created".into(),
            visibility: "private".into(),
        },
    )
    .unwrap();
    assert!(h.ai.tools.voices.get(&b).is_none());
    assert!(h.profile().voice_id.is_empty());

    drop(h);
    let h = Host::open(root.0.clone()).unwrap();
    assert_eq!(h.ai.tools.voices[&a][0].id, "fixture_voice_1");
    assert!(h.ai.tools.voices.get(&b).is_none());
}
#[test]
fn only_selected_current_package_audio_is_read() {
    let (_root, mut h, owner, hash) = fixture();
    let p = h.package.as_mut().unwrap();
    p.manifest["assets"]["voice"] = json!({"kind":"audio","path":"voice.wav"});
    p.files.insert("voice.wav".into(), vec![1, 2, 3]);
    let select = |id: &str| SampleSelection {
        asset_id: id.into(),
        transcript: "test".into(),
    };
    assert_eq!(
        prepare_samples(&h, &owner, &hash, vec![select("voice")]).unwrap()[0].bytes,
        vec![1, 2, 3]
    );
    assert!(prepare_samples(&h, &owner, &hash, vec![select("voice"), select("voice")]).is_err());
    for id in ["image", "../outside.wav", "absent"] {
        assert!(prepare_samples(&h, &owner, &hash, vec![select(id)]).is_err());
    }
    assert!(prepare_samples(&h, &owner, "stale-package", vec![select("voice")]).is_err());
}
#[test]
fn connection_uses_unsaved_key_without_persisting_it() {
    let (_root, h, _, _) = fixture();
    let mut connection = Connection {
        thinking: Default::default(),
        provider: services::Provider {
            enabled: false,
            endpoint: "https://example.org/v1".into(),
            model: "test".into(),
        },
        voice_kind: "fish".into(),
        timeout_seconds: 30,
        key: "synthetic-unsaved-key".into(),
        clear_key: false,
    };
    assert!(connection.settings("chat").unwrap().chat.enabled);
    assert_eq!(connection.key(&h, "chat").unwrap(), "synthetic-unsaved-key");
    assert!(h
        .ai
        .vault
        .key("chat", &connection.provider.endpoint)
        .unwrap()
        .is_empty());
    connection.clear_key = true;
    assert!(connection.key(&h, "chat").unwrap().is_empty());
}
#[test]
fn language_aliases_normalize_and_unlisted_values_fail() {
    let mut p = conversation::Profile {
        language: "英文".into(),
        read_language: "ja-JP".into(),
        ..Default::default()
    };
    p.normalize_languages().unwrap();
    assert_eq!(p.language, "英语");
    assert_eq!(p.read_language, "日语");
    p.read_language = "made-up-code".into();
    assert!(p.normalize_languages().is_err());
}
