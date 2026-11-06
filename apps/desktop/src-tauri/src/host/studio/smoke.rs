//! Compile-time-only native proof: real WebView/Sprite/Bubble/Web Audio receipts
//! with an isolated authoring root, no live instance and no network services.
use super::*;
pub(in crate::host) fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let result = (|| -> Result<Value, String> {
            let control = app.get_webview_window("control").ok_or("缺少控制台")?;
            std::thread::sleep(Duration::from_millis(1400));
            control
                .eval("document.querySelector('[data-tab=studio]').click()")
                .map_err(|e| e.to_string())?;
            let call = |action: &str, payload: Value| {
                tauri::async_runtime::block_on(studio_command(
                    control.clone(),
                    app.state::<Shared>(),
                    app.state::<StudioShared>(),
                    action.into(),
                    payload,
                ))
            };
            let shared = app.state::<StudioShared>().inner().clone();
            let mut p = Project::character();
            let icon = include_bytes!("../../../icons/icon.png").to_vec();
            let (w, h) = package::image_dimensions(&icon)?;
            p.asset("image.idle", "image", "idle.png", icon)?;
            p.asset("voice.test", "audio", "test.wav", tone())?;
            p.asset("voice.music", "audio", "music.wav", tone())?;
            p.document.model["logicalSize"] = json!({"width":w,"height":h});
            p.document.model["clips"]["idle"] = json!({"asset":"image.idle","layout":{"type":"single"},"direction":"forward","frameDurationMs":120});
            p.document.model["clips"]["wave"] = p.document.model["clips"]["idle"].clone();
            p.document.model["baseModes"]["music"] =
                json!({"loop":"idle","enter":"wave","exit":"wave"});
            p.document.model["baseModes"]["quiet"] = json!({"loop":"idle"});
            let routed = json!({"mode":"state-specific","stateResponses":[
                {"id":"idle-route","base":"idle","response":{"mode":"fixed","variants":[{"id":"idle-reply","text":"idle reply","audioAsset":"voice.test","animationAsset":"idle"}]}},
                {"id":"music-route","base":"music","response":{"mode":"fixed","variants":[{"id":"music-reply","text":"music reply","audioAsset":"voice.music","animationAsset":"wave"}]}},
                {"id":"quiet-route","base":"quiet","response":{"mode":"fixed","variants":[{"id":"quiet-reply","text":"quiet reply"}]}}
            ]});
            let mut missing = routed.clone();
            missing["stateResponses"].as_array_mut().unwrap().pop();
            let mut common = missing.clone();
            common["stateResponses"].as_array_mut().unwrap().push(json!({"id":"common-route","base":null,"response":{"mode":"fixed","variants":[{"id":"common-reply","text":"common reply"}]}}));
            p.document.manifest["pluginPresets"] = json!([
                {"presetId":"media","pluginId":"ayase.media-response","configVersion":"1.0","enabled":true,"priority":"normal","config":{}},
                {"presetId":"pointer","pluginId":"ayase.pointer-interaction","configVersion":"1.0","enabled":true,"priority":"normal","config":{"click":"single","response":routed}},
                {"presetId":"missing","pluginId":"ayase.pointer-interaction","configVersion":"1.0","enabled":true,"priority":"normal","config":{"click":"single","response":missing}},
                {"presetId":"common","pluginId":"ayase.pointer-interaction","configVersion":"1.0","enabled":true,"priority":"normal","config":{"click":"single","response":common}}
            ]);
            let id = shared.lock().unwrap().insert(p)?;
            let r = call(
                "preview",
                json!({"projectId":id,"response":{"mode":"fixed","variants":[{"id":"proof","text":["第一段短句。","第二段仍等待完整阅读。"],"audioAsset":["voice.test","voice.test"],"animationAsset":["wave","wave"]}]}}),
            )?;
            let first = wait(&shared, r["taskId"].as_u64().unwrap())?;
            let r = call(
                "preview",
                json!({"projectId":id,"presetId":"media","event":"environment.media","environmentActive":true,"base":"idle"}),
            )?;
            let base = wait(&shared, r["taskId"].as_u64().unwrap())?;
            let mut routed_cases = vec![];
            for mode in ["idle", "music", "quiet", "music"] {
                let task = preview_from_ui(&control, &shared, "pointer", mode)?;
                {
                    let s = shared.lock().unwrap();
                    let t = s.preview.tasks.iter().find(|t| t.id == task).unwrap();
                    let expected_audio = match mode {
                        "idle" => Some("test.wav"),
                        "music" => Some("music.wav"),
                        _ => None,
                    };
                    if t.plan["text"] != format!("{mode} reply")
                        || t.plan["stateGuard"]["base"] != mode
                        || t.plan["stateGuard"]["routeId"] != format!("{mode}-route")
                        || (mode == "music" && t.plan["clips"] != json!(["wave"]))
                        || (mode == "idle" && t.plan["clips"] != json!(["idle"]))
                        || expected_audio.is_some_and(|file| {
                            !t.plan["audioSegments"][0]["path"]
                                .as_str()
                                .is_some_and(|p| p.ends_with(file))
                        })
                    {
                        return Err(format!("界面模拟 {mode} 选择了错误回复：{}", t.plan));
                    }
                }
                routed_cases.push(
                    json!({"name":format!("UI state route {mode}"),"task":wait(&shared,task)?}),
                );
            }
            let before = shared.lock().unwrap().preview.tasks.len();
            let skipped = call(
                "preview",
                json!({"projectId":id,"presetId":"missing","base":"quiet","click":"single","event":"pointer.single"}),
            );
            if !skipped.is_err_and(|e| e.contains("没有匹配回复"))
                || shared.lock().unwrap().preview.tasks.len() != before
            {
                return Err("无匹配状态错误地生成了兜底任务".into());
            }
            routed_cases.push(json!({"name":"unmatched quiet skips without fallback"}));
            let common = preview_from_ui(&control, &shared, "common", "quiet")?;
            if shared
                .lock()
                .unwrap()
                .preview
                .tasks
                .iter()
                .find(|t| t.id == common)
                .unwrap()
                .plan["text"]
                != "common reply"
            {
                return Err("通用回复未按模拟状态匹配".into());
            }
            routed_cases.push(json!({"name":"UI common route","task":wait(&shared,common)?}));
            let export = app
                .state::<Shared>()
                .lock()
                .unwrap()
                .root
                .join("studio-created.ayase");
            call("export", json!({"projectId":id,"path":export}))?;
            let reopened = call("open-package", json!({"path":export}))?["projectId"]
                .as_str()
                .unwrap()
                .to_owned();
            let theme = call("new", json!({"kind":"theme"}))?["projectId"]
                .as_str()
                .unwrap()
                .to_owned();
            let r = call(
                "preview",
                json!({"projectId":theme,"text":"主题独立预览。"}),
            )?;
            let second = wait(&shared, r["taskId"].as_u64().unwrap())?;
            let plugin = call("new", json!({"kind":"plugin"}))?["projectId"]
                .as_str()
                .unwrap()
                .to_owned();
            let wall = chrono::Local::now()
                .date_naive()
                .and_hms_opt(23, 0, 0)
                .unwrap()
                .and_local_timezone(chrono::Local)
                .single()
                .unwrap()
                .with_timezone(&Utc)
                .to_rfc3339();
            let r = call(
                "preview",
                json!({"projectId":plugin,"wall":wall,"event":"clock.tick"}),
            )?;
            let third = wait(&shared, r["taskId"].as_u64().unwrap())?;
            let h = app.state::<Shared>();
            let h = h.lock().unwrap();
            if h.instance().is_some() || !h.scheduler.tasks.is_empty() {
                return Err("制作预览污染日常宿主".into());
            }
            let package = package::validate(&fs::read(&export).map_err(|e| e.to_string())?)?;
            if package.files.contains_key("project.json") {
                return Err("分发包泄漏制作元数据".into());
            }
            Ok(
                json!({"result":"passed","cases":[{"name":"ordered channels and reading","task":first},{"name":"media base-only preview","task":base},{"name":"theme stand-in preview","task":second},{"name":"daily plugin preview","task":third},{"name":"export and reopen","project":reopened},{"name":"runtime isolation","activeInstance":h.store.active}],"stateRoutingCases":routed_cases}),
            )
        })();
        let root = app.state::<Shared>().lock().unwrap().root.clone();
        let result = match result {
            Ok(v) => v,
            Err(e) => json!({"result":"failed","error":e}),
        };
        let _ = fs::write(
            root.join("p4-native-smoke.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        );
        app.exit(if result["result"] == "passed" { 0 } else { 1 });
    });
}
fn preview_from_ui(
    control: &WebviewWindow,
    shared: &StudioShared,
    preset: &str,
    base: &str,
) -> Result<u64, String> {
    let before = shared
        .lock()
        .unwrap()
        .preview
        .tasks
        .iter()
        .last()
        .map(|t| t.id)
        .unwrap_or(0);
    // Select by the visible field label, so this test exercises the production
    // handler rather than duplicating its selector (including the old bug).
    control.eval(&format!(r#"{{
      const panel=document.querySelector('[data-panel=studio]');
      const label=Array.from(panel.querySelectorAll('label')).find(l=>l.firstChild?.textContent==='模拟基础状态');
      label.querySelector('select').value={base};
      panel.querySelector('[data-preview-preset]').value={preset};
      panel.querySelector('[data-event]').value='pointer.single';
      panel.querySelector('[data-studio=preview-preset]').click();
    }}"#,base=serde_json::to_string(base).unwrap(),preset=serde_json::to_string(preset).unwrap())).map_err(|e|e.to_string())?;
    let at = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(80));
        let s = shared.lock().unwrap();
        if let Some(task) = s.preview.tasks.iter().find(|t| t.id > before) {
            return Ok(task.id);
        }
        if at.elapsed() > Duration::from_secs(8) {
            return Err(format!("界面未提交 {base} 模拟任务"));
        }
    }
}
fn wait(shared: &StudioShared, id: u64) -> Result<Value, String> {
    let at = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(80));
        let mut s = shared.lock().unwrap();
        s.tick();
        if let Some(t) = s.preview.tasks.iter().find(|t| t.id == id) {
            if t.status.terminal() {
                if t.status != Status::Completed {
                    return Err(format!("预览任务未完成：{:?} {:?}", t.status, t.result));
                }
                return Ok(json!({"channels":t.channels,"status":t.status,"events":s.preview.log}));
            }
        }
        if at.elapsed() > Duration::from_secs(35) {
            return Err(format!(
                "原生预览回执超时：{}",
                s.snapshot(&BTreeMap::new())
            ));
        }
    }
}
fn tone() -> Vec<u8> {
    let count = 1600u32;
    let mut b = vec![];
    b.extend(b"RIFF");
    b.extend((36 + count * 2).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(16000u32.to_le_bytes());
    b.extend(32000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend((count * 2).to_le_bytes());
    for i in 0..count {
        b.extend(
            &(((i as f64 * 440.0 * std::f64::consts::TAU / 16000.0).sin() * 800.0).round() as i16)
                .to_le_bytes(),
        );
    }
    b
}
