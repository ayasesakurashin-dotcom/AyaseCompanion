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
    assert_eq!(t.plan["streamDone"], true);
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
fn synchronized_waits_for_every_segment_even_if_later_audio_arrives_first() {
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
    assert_eq!(h.scheduler.active().unwrap().plan["audioDone"], true);
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
fn explicit_fallback_sequences_keep_one_task_and_alternate_at_actual_start() {
    let (_root, _server, mut h) = fixture();
    let response = json!({"mode":"generated-with-fallback","intent":"[fail]","fallback":[{"id":"one","text":["one-first","one-second"]},{"id":"two","text":["two-first","two-second"]}]});
    let mut previous = Value::Null;
    for _ in 0..4 {
        let id = h
            .generate(
                response.clone(),
                "test.sequence",
                Priority::Normal,
                true,
                60000,
                0,
                None,
            )
            .unwrap();
        wait(&mut h, |h| h.scheduler.active().is_some_and(|t| t.id == id));
        let task = h.scheduler.active().unwrap().clone();
        assert_ne!(task.plan["fixedChoice"]["id"], previous);
        assert_eq!(task.plan["textSegments"].as_array().unwrap().len(), 2);
        assert_ne!(task.plan["stream"], true);
        assert!(h.start_presentation(id, task.generation, h.now()));
        previous = task.plan["fixedChoice"]["id"].clone();
        // Late worker completion must not overwrite the selected fixed sequence.
        wait(&mut h, |h| h.ai.jobs.get(&id).is_some_and(|j| j.finished));
        assert_eq!(
            h.scheduler.active().unwrap().plan["textSegments"],
            task.plan["textSegments"]
        );
        assert!(h
            .scheduler
            .channel(id, task.generation, "text", false, h.now()));
        wait(&mut h, |h| !h.ai.jobs.contains_key(&id));
    }
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

#[test]
fn upstream_pauses_do_not_publish_a_partial_page_or_start_tts() {
    let (_root, server, mut h) = fixture();
    let id = submit(&mut h, "[pause] [markdown]");
    h.tick();
    std::thread::sleep(Duration::from_millis(350));
    h.tick();
    let task = h.scheduler.tasks.iter().find(|t| t.id == id).unwrap();
    assert_eq!(task.status, Status::Preparing);
    assert_eq!(task.plan["text"], "");
    assert!(server
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|r| !r["path"].as_str().unwrap().contains("tts")));
    wait(&mut h, |h| h.scheduler.active().is_some_and(|t| t.id == id));
    let text = h.scheduler.active().unwrap().plan["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(text.starts_with("合成测试摘要\n\n第一句是正常正文"));
    assert!(text.ends_with("2. 第二项内容。"));
    assert!(!text.contains("**"));
    wait(&mut h, |h| h.ai.jobs.get(&id).is_some_and(|j| j.finished));
    assert_eq!(h.scheduler.active().unwrap().plan["text"], text);
    let spoken = h.scheduler.active().unwrap().plan["audioSegments"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["spokenText"].as_str())
        .collect::<String>();
    assert_eq!(spoken, text);
    finish(&mut h);
}
#[test]
fn synchronized_does_not_start_when_only_the_first_segment_is_ready() {
    let (_root, _server, mut h) = fixture();
    h.instance_mut()
        .unwrap()
        .profile
        .as_mut()
        .unwrap()
        .presentation_policy = "speech-synchronized".into();
    let id = submit(&mut h, "[tail-slow]");
    let mut saw_first = false;
    wait(&mut h, |h| {
        let t = h.scheduler.tasks.iter().find(|t| t.id == id).unwrap();
        if !t.plan["audioSegments"][0].is_null() && t.plan["audioDone"] != true {
            saw_first = true;
            assert_eq!(t.status, Status::Preparing);
        }
        t.status == Status::Starting
    });
    assert!(saw_first);
    assert_eq!(h.scheduler.active().unwrap().plan["audioDone"], true);
    finish(&mut h);
}
#[test]
fn short_reply_limits_accept_one_and_keep_the_output_bounded() {
    for cap in [1, 20, 80] {
        let (_root, _server, mut h) = fixture();
        h.ai.settings.max_chars = cap;
        h.ai.settings.validate().unwrap();
        h.instance_mut()
            .unwrap()
            .profile
            .as_mut()
            .unwrap()
            .voice_enabled = false;
        let id = submit(&mut h, "short");
        wait(&mut h, |h| h.scheduler.active().is_some_and(|t| t.id == id));
        let n = h.scheduler.active().unwrap().plan["text"]
            .as_str()
            .unwrap()
            .chars()
            .count();
        assert!(n > 0 && n <= cap);
        finish(&mut h);
    }
    let mut s = services::Settings::default();
    s.max_chars = 0;
    assert!(s.validate().is_err());
}
#[test]
fn translated_speech_keeps_persona_and_asides_but_no_stage_directions_or_reasoning() {
    let (_root, server, mut h) = fixture();
    h.ai.settings.chat.model = "deepseek-flash".into();
    h.ai.settings.chat_thinking.effort = "high".into();
    let profile = h.instance_mut().unwrap().profile.as_mut().unwrap();
    profile.read_language = "English".into();
    profile.user_address = "Captain".into();
    profile.persona = "A playful cat companion".into();
    let id = submit(&mut h, "[speech-style]");
    wait(&mut h, |h| h.ai.jobs.get(&id).is_some_and(|j| j.finished));
    let task = h.scheduler.tasks.iter().find(|t| t.id == id).unwrap();
    assert!(task.plan["text"].as_str().unwrap().contains("过来蹭"));
    let requests = server.requests.lock().unwrap();
    let main = requests
        .iter()
        .find(|r| {
            r["body"]["messages"]
                .as_array()
                .is_some_and(|a| a.last().unwrap()["content"] == "[speech-style]")
        })
        .unwrap();
    assert_eq!(main["body"]["thinking"]["type"], "enabled");
    let translation = requests
        .iter()
        .find(|r| {
            r["body"]["messages"][0]["content"]
                .as_str()
                .is_some_and(|s| s.contains("配音翻译"))
        })
        .unwrap();
    let prompt = translation["body"]["messages"][0]["content"]
        .as_str()
        .unwrap();
    for value in ["Alpha", "Captain", "A playful cat companion"] {
        assert!(prompt.contains(value));
    }
    assert_eq!(translation["body"]["thinking"]["type"], "disabled");
    assert!(!translation["body"]["messages"][1]["content"]
        .as_str()
        .unwrap()
        .contains("过来蹭"));
    let tts = requests
        .iter()
        .find(|r| r["path"].as_str().unwrap().contains("/tts "))
        .unwrap();
    let text = tts["body"]["text"].as_str().unwrap();
    assert!(!text.contains("smiles"));
    assert!(text.contains("(Sunday)"));
    assert!(text.contains("Meow"));
}
#[test]
fn char_budget_and_upstream_length_are_diagnosed_without_speaking_a_different_body() {
    for (input, cap, reason) in [("[long]", 20, "stop"), ("[length]", 150, "length")] {
        let (_root, server, mut h) = fixture();
        h.ai.settings.max_chars = cap;
        let id = submit(&mut h, input);
        wait(&mut h, |h| h.ai.jobs.get(&id).is_some_and(|j| j.finished));
        let t = h.scheduler.tasks.iter().find(|t| t.id == id).unwrap();
        let text = t.plan["text"].as_str().unwrap();
        assert!(text.chars().count() <= cap);
        assert!(text.ends_with(['。', '…']));
        assert_eq!(t.plan["evidence"]["generation"]["finishReason"], reason);
        assert_eq!(t.plan["evidence"]["generation"]["trimmed"], true);
        let requests = server.requests.lock().unwrap();
        let speech = requests
            .iter()
            .filter(|r| r["path"].as_str().unwrap().contains("/tts "))
            .map(|r| r["body"]["text"].as_str().unwrap())
            .collect::<String>();
        assert_eq!(
            speech,
            crate::speech_text::spoken(text),
            "notice: {}; paths: {:?}",
            h.ai.notice,
            requests
                .iter()
                .map(|r| r["path"].clone())
                .collect::<Vec<_>>()
        );
        assert!(h
            .scheduler
            .log
            .iter()
            .any(|e| e["event"] == "generation-result" && e["detail"]["taskId"] == id));
    }
}
#[test]
fn state_generated_prompt_and_direct_animations_are_selected_before_services() {
    let (_root, server, mut h) = fixture();
    let model = h.package.as_mut().unwrap().model.as_mut().unwrap();
    model["baseModes"]["music"] = json!({"loop":"idle"});
    model["clips"]["anime.music_start"] = model["clips"]["idle"].clone();
    h.scheduler.claim("media", "music", Priority::Normal, false);
    let response = json!({"mode":"state-specific","stateResponses":[{"id":"idle","base":"idle","response":{"mode":"generated","intent":"idle-only-intent"}},{"id":"music","base":"music","response":{"mode":"generated","intent":"用户正在听音乐，回应用户的点击。","animationAsset":["anime.music_start","idle"],"voice":{"mode":"off"}}}]});
    let id = h
        .generate(
            response,
            "test.route",
            Priority::Normal,
            false,
            60000,
            0,
            None,
        )
        .unwrap();
    assert!(h
        .ai
        .pending
        .back()
        .unwrap()
        .input
        .contains("用户正在听音乐"));
    wait(&mut h, |h| h.ai.jobs.get(&id).is_some_and(|j| j.finished));
    let task = h
        .scheduler
        .tasks
        .iter()
        .find(|t| t.id == id)
        .unwrap()
        .clone();
    assert_eq!(task.plan["stateGuard"]["base"], "music");
    assert_eq!(task.plan["clips"], json!(["anime.music_start", "idle"]));
    let requests = server.requests.lock().unwrap();
    let body = &requests
        .iter()
        .find(|r| r["path"].as_str().unwrap().contains("chat/completions"))
        .unwrap()["body"];
    assert!(body.to_string().contains("用户正在听音乐"));
    assert!(!body.to_string().contains("idle-only-intent"));
    drop(requests);
    finish(&mut h);
}
#[test]
fn stale_state_prevents_unstarted_generation_and_explicit_fallback() {
    let (_root, server, mut h) = fixture();
    h.package.as_mut().unwrap().model.as_mut().unwrap()["baseModes"]["music"] =
        json!({"loop":"idle"});
    let response = json!({"mode":"state-specific","stateResponses":[{"id":"music","base":"music","response":{"mode":"generated-with-fallback","intent":"[fail]","fallback":[{"id":"music-fallback","text":["music one","music two"],"animationAsset":"idle"}]}}]});
    h.scheduler.claim("media", "music", Priority::Normal, false);
    let cancelled = h
        .generate(
            response.clone(),
            "test.route",
            Priority::Normal,
            false,
            60000,
            0,
            None,
        )
        .unwrap();
    h.scheduler.claims.clear();
    h.tick();
    assert!(server.requests.lock().unwrap().is_empty());
    assert_eq!(
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == cancelled)
            .unwrap()
            .status,
        Status::Cancelled
    );
    h.scheduler.claim("media", "music", Priority::Normal, false);
    let id = h
        .generate(
            response,
            "test.route",
            Priority::Normal,
            false,
            60000,
            0,
            None,
        )
        .unwrap();
    wait(&mut h, |h| h.scheduler.active().is_some_and(|t| t.id == id));
    let task = h.scheduler.active().unwrap().clone();
    assert_eq!(task.plan["textSegments"], json!(["music one", "music two"]));
    assert_eq!(task.plan["stateGuard"]["routeId"], "music");
    h.scheduler.claims.clear();
    assert!(!h.start_presentation(id, task.generation, h.now()));
    assert_eq!(
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .status,
        Status::Cancelled
    );
    finish(&mut h);
}
#[test]
fn fixed_observation_reply_prepares_evidence_without_calling_chat_or_tts() {
    let (_root, server, mut h) = fixture();
    h.instance_mut().unwrap().overrides["extraPresets"].as_array_mut().unwrap().push(json!({"presetId":"test.observation","pluginId":"ayase.observation","configVersion":"1.0","enabled":false,"config":{"reply":true}}));
    let id=h.generate(json!({"mode":"fixed","variants":[{"id":"fixed","text":"prepared fixed reply","animationAsset":"idle"}]}),"test.observation",Priority::Normal,false,60000,0,None).unwrap();
    assert!(h.ai.pending.iter().any(|s| s.id == id));
    wait(&mut h, |h| h.scheduler.active().is_some_and(|t| t.id == id));
    let task = h.scheduler.active().unwrap();
    assert_eq!(task.plan["text"], "prepared fixed reply");
    assert!(task.plan["evidence"].is_object());
    assert_ne!(task.plan["stream"], true);
    assert!(server.requests.lock().unwrap().is_empty());
    finish(&mut h);
}
