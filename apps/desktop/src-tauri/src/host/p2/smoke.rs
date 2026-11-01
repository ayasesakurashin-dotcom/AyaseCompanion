use super::*;
use crate::test_http::Server;
fn wait(
    state: &Shared,
    seconds: u64,
    mut predicate: impl FnMut(&Host) -> bool,
) -> Result<(), String> {
    let start = Instant::now();
    loop {
        if predicate(&state.lock().unwrap()) {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            return Err("原生 P2 检查超时".into());
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
fn task_done(h: &Host, id: u64) -> bool {
    h.scheduler
        .tasks
        .iter()
        .any(|t| t.id == id && t.status.terminal())
}
pub(super) fn start(app: tauri::AppHandle, state: Shared) {
    std::thread::spawn(move || {
        let server = Server::start();
        let mut evidence = vec![];
        let result = (|| -> Result<(), String> {
            let package = std::env::var("AYASE_SMOKE_PACKAGE").map_err(|_| "缺少隔离测试角色包")?;
            let (a, hash) = {
                let mut h = state.lock().unwrap();
                let bytes = fs::read(package).map_err(|e| e.to_string())?;
                let report = h.stage_archive(&bytes)?;
                activate(&mut h, &report.hash, "new")?;
                allow_assets(&app, &h);
                let id = h.instance().unwrap().id.clone();
                let mut store = h.store.clone();
                store.bubble.animation = "none".into();
                store.bubble.read_ms = 1000;
                h.commit(store)?;
                h.instance_mut().unwrap().profile = Some(conversation::Profile {
                    character_name: "Alpha".into(),
                    persona: "Kind and concise".into(),
                    voice_enabled: true,
                    voice_id: "fixture-voice".into(),
                    ..Default::default()
                });
                h.ai.settings.chat = services::Provider {
                    enabled: true,
                    endpoint: format!("{}/v1", server.endpoint),
                    model: "fixture".into(),
                };
                h.ai.settings.voice = services::Provider {
                    enabled: true,
                    endpoint: format!("{}/v1", server.endpoint),
                    model: "fixture".into(),
                };
                h.ai.settings.memory.enabled = false;
                (id, report.hash)
            };
            wait(&state, 20, |h| h.renderer_ready)?;
            wait(&state, 30, |h| {
                h.scheduler.tasks.iter().all(|t| t.status.terminal())
            })?;
            let first = {
                let mut h = state.lock().unwrap();
                h.generate(
                    json!({"mode":"generated","intent":"[long]","cue":"greeting"}),
                    "smoke.generated",
                    Priority::Background,
                    false,
                    120000,
                    0,
                    Some("[long] [pause] [markdown]".into()),
                )?
            };
            let mut text_before_audio = false;
            wait(&state, 15, |h| {
                if let Some(t) = h.scheduler.active().filter(|t| t.id == first) {
                    if t.status == Status::Presenting && t.plan["audioSegments"][0].is_null() {
                        text_before_audio = true;
                    }
                    t.status == Status::Presenting || t.status.terminal()
                } else {
                    false
                }
            })?;
            if state
                .lock()
                .unwrap()
                .scheduler
                .tasks
                .iter()
                .find(|t| t.id == first)
                .is_some_and(|t| t.status.terminal())
            {
                return Err(state.lock().unwrap().ai.notice.clone());
            }
            let high = {
                let mut h = state.lock().unwrap();
                h.present(json!({"id":"important","text":"这条重要任务应等待前一条回复完整结束。","readMs":1000}),"smoke.important",Priority::Important,false,120000,0)?
            };
            wait(&state, 55, |h| task_done(h, first) && task_done(h, high))?;
            {
                let h = state.lock().unwrap();
                let first = h.scheduler.tasks.iter().find(|t| t.id == first).unwrap();
                let high_task = h.scheduler.tasks.iter().find(|t| t.id == high).unwrap();
                if first.status != Status::Completed || high_task.status != Status::Completed {
                    return Err("文字、音声或动作未正常收尾".into());
                }
                let acks = first.plan["audioAcks"]
                    .as_array()
                    .ok_or("缺少实际音频片段回执")?;
                if acks.len() < 2 {
                    return Err("未形成多段音声".into());
                }
                let mut previous = 0;
                for ack in acks {
                    let start = ack["startedAt"].as_u64().ok_or("音频未实际开始")?;
                    let end = ack["endedAt"].as_u64().ok_or("音频未实际结束")?;
                    if start < previous
                        || end.saturating_sub(start) < 750
                        || ack["result"] != "completed"
                    {
                        return Err("音频顺序或真实持续时间异常".into());
                    }
                    previous = end;
                }
                let finish_at = h
                    .scheduler
                    .log
                    .iter()
                    .find(|l| l["event"] == "finished" && l["detail"]["taskId"] == first.id)
                    .and_then(|l| l["atMs"].as_u64())
                    .ok_or("缺少任务结束")?;
                let high_started = h
                    .scheduler
                    .log
                    .iter()
                    .find(|l| l["event"] == "started" && l["detail"]["taskId"] == high)
                    .and_then(|l| l["atMs"].as_u64())
                    .ok_or("重要任务未开始")?;
                if high_started < finish_at {
                    return Err("重要任务抢占了当前回复".into());
                }
                let history = h.ai.memory.history(&a)?;
                if !history["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["role"] == "assistant" && m["status"] == "completed")
                {
                    return Err("实际呈现记账缺失".into());
                }
                evidence.push(json!({"case":"text-first ordered audio and full handoff","passed":true,"textBeforeAudio":text_before_audio,"segments":acks,"historyCount":history["messages"].as_array().unwrap().len()}));
            }
            if !text_before_audio {
                return Err("文字优先未在音声就绪前开始".into());
            }
            {
                let h = state.lock().unwrap();
                let first = h.scheduler.tasks.iter().find(|t| t.id == first).unwrap();
                let text = first.plan["text"].as_str().unwrap_or("");
                if first.plan["streamDone"] != true
                    || text.contains("**")
                    || text.starts_with('#')
                    || !text.contains("2. 第二项内容。")
                {
                    return Err("正文未完整可读化".into());
                }
                let acks = first.plan["audioAcks"].as_array().unwrap();
                let gaps = acks
                    .windows(2)
                    .filter_map(|a| {
                        Some(
                            a[1]["startedAt"]
                                .as_u64()?
                                .saturating_sub(a[0]["endedAt"].as_u64()?),
                        )
                    })
                    .collect::<Vec<_>>();
                evidence.push(json!({"case":"paused Markdown source fully buffered before local reveal; single-context audio","passed":true,"audioGapsMs":gaps,"fullTextBeforePresentation":true}));
            }
            let second = {
                let mut h = state.lock().unwrap();
                h.instance_mut()
                    .unwrap()
                    .profile
                    .as_mut()
                    .unwrap()
                    .presentation_policy = "speech-synchronized".into();
                h.generate(
                    json!({"mode":"generated","intent":"sync"}),
                    "smoke.generated",
                    Priority::Background,
                    false,
                    120000,
                    0,
                    Some("sync".into()),
                )?
            };
            wait(&state, 15, |h| {
                h.scheduler.active().is_some_and(|t| t.id == second)
            })?;
            {
                let h = state.lock().unwrap();
                if h.scheduler.active().unwrap().plan["audioSegments"][0]["path"].is_null() {
                    return Err("同步策略未等待首段音声".into());
                }
                if h.scheduler.active().unwrap().plan["audioDone"] != true {
                    return Err("同步策略未等待全部音声准备完成".into());
                }
            }
            wait(&state, 35, |h| task_done(h, second))?;
            {
                let h = state.lock().unwrap();
                let task = h.scheduler.tasks.iter().find(|t| t.id == second).unwrap();
                if task.status != Status::Completed
                    || task.channels.get("audio").is_none_or(|s| s != "completed")
                {
                    return Err("同步回复未正常结束".into());
                }
                evidence.push(json!({"case":"speech-synchronized actual callbacks","passed":true,"channels":task.channels}));
            }
            // Exercise the utility preparation/completion path with real Web Audio.
            let (preview, reply, before_count) = {
                let mut h = state.lock().unwrap();
                let before_count = h.ai.memory.history(&a)?["messages"]
                    .as_array()
                    .unwrap()
                    .len();
                let reply = h.present(
                    json!({"text":"试听等待此回复完整结束。","readMs":1500}),
                    "host.user",
                    Priority::User,
                    false,
                    60000,
                    0,
                )?;
                (tools::prepare_preview(&mut h, &a, 60)?, reply, before_count)
            };
            wait(&state, 10, |h| {
                h.scheduler.active().is_some_and(|t| t.id == reply)
            })?;
            let settings = state.lock().unwrap().ai.settings.clone();
            let bytes = services::speech(&settings, "", "试听原文", "fixture-voice")?;
            {
                let mut h = state.lock().unwrap();
                tools::complete_preview(&mut h, &a, preview.0, preview.1, &preview.2, Ok(bytes))?;
                if h.scheduler.active().is_none_or(|t| t.id != reply) {
                    return Err("试听抢占了现有回复".into());
                }
            }
            wait(&state, 15, |h| task_done(h, preview.0))?;
            {
                let h = state.lock().unwrap();
                let task = h
                    .scheduler
                    .tasks
                    .iter()
                    .find(|t| t.id == preview.0)
                    .unwrap();
                if task.status != Status::Completed
                    || task.channels.get("audio").is_none_or(|s| s != "completed")
                {
                    return Err("试听未得到实际音频完成回执".into());
                }
                if h.ai.memory.history(&a)?["messages"]
                    .as_array()
                    .unwrap()
                    .len()
                    != before_count
                {
                    return Err("试听写入了对话历史".into());
                }
                evidence.push(json!({"case":"voice audition actual Web Audio, no preemption or history","passed":true,"channels":task.channels}));
            }
            let cancelled = {
                let mut h = state.lock().unwrap();
                h.generate(
                    json!({"mode":"generated","intent":"[slow]"}),
                    "host.user",
                    Priority::User,
                    false,
                    120000,
                    0,
                    Some("[slow] late alpha input".into()),
                )?
            };
            std::thread::sleep(Duration::from_millis(100));
            let b = {
                let mut h = state.lock().unwrap();
                activate(&mut h, &hash, "new")?;
                allow_assets(&app, &h);
                h.instance().unwrap().id.clone()
            };
            wait(&state, 10, |h| h.ai.jobs.is_empty() && h.renderer_ready)?;
            {
                let h = state.lock().unwrap();
                let history = h.ai.memory.history(&b)?;
                if !history["messages"].as_array().unwrap().is_empty()
                    || h.ai.memory.bank(&a)? == h.ai.memory.bank(&b)?
                {
                    return Err("伙伴隔离失效".into());
                }
                if h.scheduler
                    .tasks
                    .iter()
                    .find(|t| t.id == cancelled)
                    .is_some_and(|t| !t.status.terminal())
                {
                    return Err("旧生成任务未取消".into());
                }
                evidence.push(json!({"case":"switch during generation","passed":true,"newInstanceHistoryEmpty":true,"independentBank":true}));
            }
            let partial = {
                let mut h = state.lock().unwrap();
                h.generate(
                    json!({"mode":"generated","intent":"[cut]"}),
                    "host.user",
                    Priority::User,
                    false,
                    120000,
                    0,
                    Some("[cut]".into()),
                )?
            };
            wait(&state, 35, |h| {
                task_done(h, partial) && !h.ai.jobs.contains_key(&partial)
            })?;
            {
                let h = state.lock().unwrap();
                let task = h.scheduler.tasks.iter().find(|t| t.id == partial).unwrap();
                if task.channels.get("text").is_none_or(|s| s != "failed") {
                    return Err("断流文字未按故障收尾".into());
                }
                let history = h.ai.memory.history(&b)?;
                if !history["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["role"] == "assistant" && m["status"] == "partial-final")
                {
                    return Err("断流回复未记录实际呈现部分".into());
                }
                evidence.push(json!({"case":"interrupted stream closes and records only shown text","passed":true,"channels":task.channels}));
            }
            // One fixed task owns all text materials, ordered recordings and cues.
            let settings = state.lock().unwrap().ai.settings.clone();
            let bytes =
                services::speech(&settings, "", "synthetic fixed recording", "fixture-voice")?;
            let sequence = {
                let mut h = state.lock().unwrap();
                let package = h.package.as_ref().unwrap();
                let path = h
                    .root
                    .join("packages")
                    .join(&package.report.hash)
                    .join("content/smoke-sequence.wav");
                fs::write(&path, &bytes).map_err(|e| e.to_string())?;
                let package = h.package.as_mut().unwrap();
                package.files.insert("smoke-sequence.wav".into(), bytes);
                package.manifest["assets"]["smoke.recording"] =
                    json!({"kind":"audio","path":"smoke-sequence.wav","durationMs":1000});
                allow_assets(&app, &h);
                h.present_variants(json!({"mode":"fixed","variants":[{"id":"sequence","text":["第一段。","Second material.","最后一段。"],"audioAsset":"smoke.recording,smoke.recording\nsmoke.recording","cue":["greeting","greeting"],"readMs":1000}]}),"smoke.fixed-sequence",Priority::Normal,false,120000,0)?
            };
            wait(&state, 10, |h| {
                h.scheduler
                    .active()
                    .is_some_and(|t| t.id == sequence && t.status == Status::Presenting)
            })?;
            let high = {
                let mut h = state.lock().unwrap();
                h.present(
                    json!({"id":"after-sequence","text":"next","readMs":1000}),
                    "smoke.after-sequence",
                    Priority::Important,
                    false,
                    120000,
                    0,
                )?
            };
            wait(&state, 45, |h| task_done(h, sequence) && task_done(h, high))?;
            {
                let h = state.lock().unwrap();
                let task = h.scheduler.tasks.iter().find(|t| t.id == sequence).unwrap();
                let acks = task.plan["audioAcks"]
                    .as_array()
                    .ok_or("多固定录音缺少真实回执")?;
                if task.status != Status::Completed
                    || task.channels.len() != 3
                    || acks.len() != 3
                    || acks.iter().any(|v| v["result"] != "completed")
                    || acks.windows(2).any(|p| {
                        p[1]["startedAt"].as_u64().unwrap_or(0)
                            < p[0]["endedAt"].as_u64().unwrap_or(u64::MAX)
                    })
                {
                    return Err("固定素材序列的通道或片段顺序异常".into());
                }
                let finish = h
                    .scheduler
                    .log
                    .iter()
                    .find(|v| v["event"] == "finished" && v["detail"]["taskId"] == sequence)
                    .and_then(|v| v["atMs"].as_u64())
                    .ok_or("序列未结束")?;
                let next = h
                    .scheduler
                    .log
                    .iter()
                    .find(|v| v["event"] == "started" && v["detail"]["taskId"] == high)
                    .and_then(|v| v["atMs"].as_u64())
                    .ok_or("后续任务未开始")?;
                if next < finish {
                    return Err("多素材任务被后续任务抢占".into());
                }
                evidence.push(json!({"case":"fixed multi-material sequence shares text audio animation ownership; later important waits","passed":true,"channels":task.channels,"textSegments":task.plan["textSegments"],"clips":task.plan["clips"],"audioAcks":acks,"nextWaited":true}));
            }
            let minimum_reading = {
                let mut h = state.lock().unwrap();
                let mut store = h.store.clone();
                store.bubble.read_ms = 3000;
                h.commit(store)?;
                h.present(
                    json!({"id":"minimum-reading","text":["甲","乙"],"readMs":1000}),
                    "smoke.minimum-reading",
                    Priority::Normal,
                    false,
                    120000,
                    0,
                )?
            };
            wait(&state, 20, |h| task_done(h, minimum_reading))?;
            {
                let mut h = state.lock().unwrap();
                let task = h
                    .scheduler
                    .tasks
                    .iter()
                    .find(|t| t.id == minimum_reading)
                    .unwrap();
                let start = h
                    .scheduler
                    .log
                    .iter()
                    .find(|v| v["event"] == "started" && v["detail"]["taskId"] == minimum_reading)
                    .and_then(|v| v["atMs"].as_u64())
                    .ok_or("短文未开始")?;
                let end = h
                    .scheduler
                    .log
                    .iter()
                    .find(|v| v["event"] == "finished" && v["detail"]["taskId"] == minimum_reading)
                    .and_then(|v| v["atMs"].as_u64())
                    .ok_or("短文未结束")?;
                if task.status != Status::Completed || end - start < 6000 {
                    return Err("短素材未遵守每段3000毫秒的全局阅读下限".into());
                }
                evidence.push(json!({"case":"every short fixed material honors global minimum despite smaller task readMs","passed":true,"globalReadMs":3000,"taskReadMs":1000,"textMaterials":2,"elapsedMs":end-start}));
                let mut store = h.store.clone();
                store.bubble.read_ms = 1000;
                h.commit(store)?;
            }
            let routed = {
                let mut h = state.lock().unwrap();
                h.scheduler
                    .claim("smoke.state", "music", Priority::Normal, false);
                h.generate(json!({"mode":"state-specific","stateResponses":[{"id":"idle","base":"idle","response":{"mode":"fixed","variants":[{"id":"idle","text":"idle"}]}},{"id":"music","base":"music","response":{"mode":"fixed","variants":[{"id":"music","text":["music first","music second"],"animationAsset":["music_start","music_end"],"audioAsset":["smoke.recording","smoke.recording"],"readMs":1000}]}}]}),"smoke.routed",Priority::Normal,false,120000,0,None)?
            };
            wait(&state, 10, |h| {
                h.scheduler
                    .active()
                    .is_some_and(|t| t.id == routed && t.status == Status::Presenting)
            })?;
            {
                let mut h = state.lock().unwrap();
                h.scheduler.claims.remove("smoke.state");
            }
            wait(&state, 40, |h| task_done(h, routed))?;
            {
                let mut h = state.lock().unwrap();
                let task = h.scheduler.tasks.iter().find(|t| t.id == routed).unwrap();
                if task.status != Status::Completed
                    || task.plan["stateGuard"]["routeId"] != "music"
                    || task.plan["clips"] != json!(["music_start", "music_end"])
                    || task.channels.values().any(|v| v != "completed")
                {
                    return Err("状态专用回复、直接动画序列或开始后非抢占保护失败".into());
                }
                evidence.push(json!({"case":"state-specific fixed reply plays direct animation clips and survives later state change","passed":true,"stateGuard":task.plan["stateGuard"],"clips":task.plan["clips"],"channels":task.channels}));
                h.scheduler
                    .claim("smoke.state", "quiet", Priority::Important, false);
                if h.select_response(
                    &json!({"mode":"state-specific","stateResponses":[]}),
                    "smoke.no-route",
                )
                .is_ok()
                {
                    return Err("无状态匹配时错误兜底".into());
                }
                h.scheduler.claims.remove("smoke.state");
            }
            let hit_regions = crate::host::hit_testing::diagnostics(&app);
            if hit_regions["regionCount"].as_u64().unwrap_or(0) < 2
                || hit_regions["updatedRecently"] != true
            {
                return Err("原生模型/工具栏命中区域未收到有效前端回报".into());
            }
            evidence.push(json!({"case":"native model and toolbar input regions received from actual rendered frames","passed":true,"regions":hit_regions,"note":"does not replace manual cross-window pointer/DPI/drag acceptance"}));
            Ok(())
        })();
        let h = state.lock().unwrap();
        let requests = server.requests.lock().unwrap();
        let report = json!({"result":if result.is_ok(){"passed"}else{"failed"},"error":result.as_ref().err(),"serviceNotice":h.ai.notice,"fixtureEndpoint":server.endpoint,"cases":evidence,"log":h.scheduler.log,"tasks":h.scheduler.tasks,"serviceMode":"synthetic local HTTP fixture; real WebView2, Canvas and Web Audio","requests":requests.len(),"syntheticRequestPaths":requests.iter().map(|r|&r["path"]).collect::<Vec<_>>()});
        let _ = fs::write(
            h.root.join("p2-native-smoke.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        );
        drop(h);
        app.exit(if result.is_ok() { 0 } else { 1 });
    });
}
