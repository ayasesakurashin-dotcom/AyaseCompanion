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
            p.document.model["logicalSize"] = json!({"width":w,"height":h});
            p.document.model["clips"]["idle"] = json!({"asset":"image.idle","layout":{"type":"single"},"direction":"forward","frameDurationMs":120});
            p.document.model["clips"]["wave"] = p.document.model["clips"]["idle"].clone();
            p.document.model["baseModes"]["music"] =
                json!({"loop":"idle","enter":"wave","exit":"wave"});
            p.document.manifest["pluginPresets"] = json!([{"presetId":"media","pluginId":"ayase.media-response","configVersion":"1.0","enabled":true,"priority":"normal","config":{}}]);
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
                json!({"result":"passed","cases":[{"name":"ordered channels and reading","task":first},{"name":"media base-only preview","task":base},{"name":"theme stand-in preview","task":second},{"name":"daily plugin preview","task":third},{"name":"export and reopen","project":reopened},{"name":"runtime isolation","activeInstance":h.store.active}]}),
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
