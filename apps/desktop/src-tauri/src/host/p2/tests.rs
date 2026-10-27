use super::*;
use crate::{host::tests as baseline, test_http::Server};
fn fixture() -> (baseline::TestRoot, Server, Host) {
    let root = baseline::TestRoot::new();
    let server = Server::start();
    let mut h = Host::open(root.0.clone()).unwrap();
    let p = h.stage_archive(&baseline::archive("0.1.0")).unwrap();
    activate(&mut h, &p.hash, "new").unwrap();
    h.renderer_ready = true;
    let mut s = services::Settings::default();
    s.chat = services::Provider {
        enabled: true,
        endpoint: format!("{}/v1", server.endpoint),
        model: "fixture".into(),
    };
    s.voice = services::Provider {
        enabled: true,
        endpoint: format!("{}/v1", server.endpoint),
        model: "fixture".into(),
    };
    h.ai.vault
        .save(
            &s,
            &json!({"chat":"synthetic-chat-key","voice":"synthetic-voice-key"}),
        )
        .unwrap();
    h.ai.settings = s;
    h.instance_mut().unwrap().profile = Some(conversation::Profile {
        character_name: "Alpha".into(),
        persona: "Use short kind sentences".into(),
        voice_enabled: true,
        voice_id: "synthetic-voice".into(),
        ..Default::default()
    });
    (root, server, h)
}
fn wait(h: &mut Host, mut ready: impl FnMut(&Host) -> bool) {
    let started = Instant::now();
    while !ready(h) {
        h.tick();
        std::thread::sleep(Duration::from_millis(15));
        assert!(
            started.elapsed() < Duration::from_secs(12),
            "P2 fixture timeout; {}",
            h.ai.notice
        );
    }
}
fn submit(h: &mut Host, input: &str) -> u64 {
    h.generate(
        json!({"mode":"generated","intent":"chat"}),
        "host.user",
        Priority::User,
        false,
        60000,
        0,
        Some(input.into()),
    )
    .unwrap()
}
fn finish(h: &mut Host) {
    h.cancel_generations();
    wait(h, |h| h.ai.jobs.is_empty());
}
#[test]
fn text_first_starts_before_audio_and_never_exposes_reasoning() {
    let (_root, server, mut h) = fixture();
    let id = submit(&mut h, "normal");
    wait(&mut h, |h| h.scheduler.active().is_some_and(|t| t.id == id));
    let t = h.scheduler.active().unwrap();
    assert!(t.plan["audioSegments"][0].is_null());
    assert!(!t.plan["text"].as_str().unwrap().contains("hidden"));
    wait(&mut h, |h| h.ai.jobs.get(&id).is_some_and(|j| j.finished));
    let requests = server.requests.lock().unwrap();
    for r in requests.iter() {
        let auth = r["authorization"].as_str().unwrap();
        if r["path"].as_str().unwrap().contains("tts") {
            assert!(auth.contains("synthetic-voice-key"));
            assert!(!auth.contains("synthetic-chat-key"));
        } else {
            assert!(auth.contains("synthetic-chat-key"));
            assert!(r["body"]["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("Alpha"));
        }
    }
    drop(requests);
    finish(&mut h);
}
#[test]
fn synchronized_waits_for_first_segment_even_if_later_audio_arrives_first() {
    let (_root, _server, mut h) = fixture();
    h.instance_mut()
        .unwrap()
        .profile
        .as_mut()
        .unwrap()
        .presentation_policy = "speech-synchronized".into();
    let id = submit(&mut h, "normal");
    let mut out_of_order = false;
    wait(&mut h, |h| {
        let t = h.scheduler.tasks.iter().find(|t| t.id == id).unwrap();
        if !t.plan["audioSegments"][1].is_null() && t.plan["audioSegments"][0].is_null() {
            out_of_order = true;
            assert!(h.scheduler.active().is_none());
        }
        t.status == Status::Starting
    });
    assert!(out_of_order);
    assert!(!h.scheduler.active().unwrap().plan["audioSegments"][0].is_null());
    finish(&mut h);
}
#[test]
fn stop_during_preparation_rejects_late_output_and_assistant_memory() {
    let (_root, _server, mut h) = fixture();
    let instance = h.instance().unwrap().id.clone();
    let id = submit(&mut h, "[slow]");
    h.tick();
    h.cancel_generations();
    wait(&mut h, |h| h.ai.jobs.is_empty());
    let t = h.scheduler.tasks.iter().find(|t| t.id == id).unwrap();
    assert_eq!(t.status, Status::Cancelled);
    let history = h.ai.memory.history(&instance).unwrap();
    assert_eq!(history["messages"].as_array().unwrap().len(), 1);
    assert_eq!(history["messages"][0]["role"], "user");
}
#[test]
fn generated_failure_uses_only_explicit_fallback() {
    let (_root, _server, mut h) = fixture();
    let id=h.generate(json!({"mode":"generated-with-fallback","intent":"[fail]","fallback":[{"id":"fallback","text":"明确配置的后备"}]}),"greeting.test",Priority::Normal,true,60000,0,None).unwrap();
    wait(&mut h, |h| h.scheduler.active().is_some_and(|t| t.id == id));
    assert_eq!(h.scheduler.active().unwrap().plan["text"], "明确配置的后备");
    assert_ne!(h.scheduler.active().unwrap().plan["stream"], true);
    let now = h.now();
    h.scheduler.stop(now);
    wait(&mut h, |h| h.ai.jobs.is_empty());
    let id = submit(&mut h, "[fail]");
    wait(&mut h, |h| {
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .status
            .terminal()
    });
    assert_eq!(
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .status,
        Status::Failed
    );
    finish(&mut h);
}
#[test]
fn stop_also_revokes_a_generated_tasks_prepared_fixed_fallback() {
    let (_root, _server, mut h) = fixture();
    let high = h
        .present(
            json!({"id":"manual","text":"foreground"}),
            "host.user",
            Priority::User,
            false,
            60000,
            0,
        )
        .unwrap();
    h.tick();
    let t = h.scheduler.active().unwrap().clone();
    assert_eq!(t.id, high);
    let now = h.now();
    h.scheduler.started(t.id, t.generation, now);
    let fallback=h.generate(json!({"mode":"generated-with-fallback","intent":"[fail]","fallback":[{"id":"fallback","text":"prepared fallback"}]}),"test.plugin",Priority::Normal,true,60000,0,None).unwrap();
    wait(&mut h, |h| {
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == fallback)
            .is_some_and(|t| t.status == Status::Ready)
    });
    h.cancel_generations();
    assert_eq!(
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == fallback)
            .unwrap()
            .status,
        Status::Cancelled
    );
    assert_eq!(h.scheduler.active().unwrap().id, high);
    finish(&mut h);
}
#[test]
fn switching_same_package_fences_pending_generation_and_context() {
    let (_root, _server, mut h) = fixture();
    let a = h.instance().unwrap().id.clone();
    let hash = h.package.as_ref().unwrap().report.hash.clone();
    submit(&mut h, "[slow] A-only input");
    h.tick();
    activate(&mut h, &hash, "new").unwrap();
    let b = h.instance().unwrap().id.clone();
    assert_ne!(a, b);
    assert_ne!(h.ai.memory.bank(&a).unwrap(), h.ai.memory.bank(&b).unwrap());
    wait(&mut h, |h| h.ai.jobs.is_empty());
    assert_eq!(
        h.ai.memory.history(&b).unwrap()["messages"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert!(h
        .ai
        .memory
        .history(&a)
        .unwrap()
        .to_string()
        .contains("A-only"));
    assert!(!h
        .ai
        .memory
        .history(&b)
        .unwrap()
        .to_string()
        .contains("A-only"));
}
#[test]
fn preparation_does_not_take_the_foreground_lock() {
    let (_root, _server, mut h) = fixture();
    let low = h
        .generate(
            json!({"mode":"generated","intent":"[slow]"}),
            "background.test",
            Priority::Background,
            true,
            60000,
            0,
            None,
        )
        .unwrap();
    let high = h
        .present(
            json!({"id":"high","text":"important"}),
            "important.test",
            Priority::Important,
            true,
            60000,
            0,
        )
        .unwrap();
    h.tick();
    let generation = h.scheduler.active().unwrap().generation;
    assert_eq!(h.scheduler.active().unwrap().id, high);
    let now = h.now();
    assert!(h.scheduler.started(high, generation, now));
    wait(&mut h, |h| {
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == low)
            .unwrap()
            .status
            == Status::Ready
    });
    assert_eq!(h.scheduler.active().unwrap().id, high);
    let now = h.now();
    h.scheduler.channel(high, generation, "text", false, now);
    wait(&mut h, |h| {
        h.scheduler.active().is_some_and(|t| t.id == low)
    });
    finish(&mut h);
}

#[test]
fn changed_profile_revokes_an_unstarted_lease_but_not_started_output() {
    let (_root, _server, mut h) = fixture();
    let id = submit(&mut h, "normal");
    wait(&mut h, |h| h.scheduler.active().is_some_and(|t| t.id == id));
    h.instance_mut().unwrap().profile_revision += 1;
    assert!(!h.generation_start_valid(id));
    assert_eq!(
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .status,
        Status::Cancelled
    );
    wait(&mut h, |h| h.ai.jobs.is_empty());
    let id = submit(&mut h, "normal");
    wait(&mut h, |h| h.scheduler.active().is_some_and(|t| t.id == id));
    let t = h.scheduler.active().unwrap().clone();
    let now = h.now();
    assert!(h.scheduler.started(t.id, t.generation, now));
    h.instance_mut().unwrap().profile_revision += 1;
    assert!(h.generation_start_valid(id));
    h.tick();
    assert_eq!(h.scheduler.active().unwrap().id, id);
    finish(&mut h);
}

#[test]
fn partial_stream_error_never_replaces_visible_text_with_fallback() {
    let (_root, _server, mut h) = fixture();
    let id=h.generate(json!({"mode":"generated-with-fallback","intent":"[cut]","fallback":[{"id":"fallback","text":"must-not-replace-partial"}]}),"test.plugin",Priority::Normal,true,60000,0,None).unwrap();
    wait(&mut h, |h| h.ai.jobs.get(&id).is_some_and(|j| j.finished));
    let t = h.scheduler.tasks.iter().find(|t| t.id == id).unwrap();
    assert!(t.plan["streamDone"] == true);
    assert!(!t.plan["text"]
        .as_str()
        .unwrap()
        .contains("must-not-replace-partial"));
    assert!(!t.plan["text"].as_str().unwrap().is_empty());
    finish(&mut h);
}

#[test]
fn materialized_export_preserves_unknown_identity_and_asset_bytes_without_local_bindings() {
    let (_root, _server, mut h) = fixture();
    let mut manifest = h.package.as_ref().unwrap().manifest.clone();
    manifest["identity"] = json!({"personaAsset":"original","unknownAuthorField":{"x":42}});
    manifest["assets"]["voice"] = json!({"kind":"audio","path":"voice.wav"});
    let overrides = json!({"identity":{"characterName":"Exported","persona":"authored persona"},"assetEnabled":{"voice":false}});
    materialize_identity_assets(&mut manifest, &overrides);
    assert_eq!(manifest["identity"]["unknownAuthorField"]["x"], 42);
    assert!(manifest["identity"].get("personaAsset").is_none());
    assert_eq!(manifest["assets"]["voice"]["enabled"], false);
    let text = manifest.to_string();
    for forbidden in ["voiceId", "bankId", "instanceId", "apiKey", "syncMemory"] {
        assert!(!text.contains(forbidden));
    }
    finish(&mut h);
}
