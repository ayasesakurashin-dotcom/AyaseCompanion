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
                    Some("[long]".into()),
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
            }
            wait(&state, 35, |h| task_done(h, second))?;
            {
                let h = state.lock().unwrap();
                let task = h.scheduler.tasks.iter().find(|t| t.id == second).unwrap();
                if task.status != Status::Completed {
                    return Err("同步回复未正常结束".into());
                }
                evidence.push(json!({"case":"speech-synchronized actual callbacks","passed":true,"channels":task.channels}));
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
            Ok(())
        })();
        let h = state.lock().unwrap();
        let report = json!({"result":if result.is_ok(){"passed"}else{"failed"},"error":result.as_ref().err(),"serviceNotice":h.ai.notice,"fixtureEndpoint":server.endpoint,"cases":evidence,"log":h.scheduler.log,"tasks":h.scheduler.tasks,"serviceMode":"synthetic local HTTP fixture; real WebView2, Canvas and Web Audio","requests":server.requests.lock().unwrap().len()});
        let _ = fs::write(
            h.root.join("p2-native-smoke.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        );
        drop(h);
        app.exit(if result.is_ok() { 0 } else { 1 });
    });
}
