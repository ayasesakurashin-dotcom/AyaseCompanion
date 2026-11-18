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
            for (name, color) in [
                ("blue", [0, 0, 255, 255]),
                ("red", [255, 0, 0, 255]),
                ("yellow", [255, 255, 0, 255]),
                ("green", [0, 255, 0, 255]),
            ] {
                let mut bytes = std::io::Cursor::new(vec![]);
                image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                    8,
                    8,
                    image::Rgba(color),
                ))
                .write_to(&mut bytes, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
                p.asset(name, "image", &format!("{name}.png"), bytes.into_inner())?;
            }
            p.document.model["decorationVersion"] = json!("1.0");
            p.document.model["decorations"]=json!([("blue","background",1.4,0),("red","foreground",0.5,0),("yellow","foreground",0.25,1)].iter().map(|(id,side,factor,order)|json!({"id":id,"name":id,"side":side,"order":order,"enabled":true,"source":{"type":"image","asset":id},"anchor":{"x":0.5,"y":0.5},"pivot":{"x":0.5,"y":0.5},"offset":{"x":0,"y":0},"width":w as f64*factor,"height":h as f64*factor,"keepAspect":true,"scale":1,"speed":1,"states":null})).collect::<Vec<_>>());
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
            call(
                "preview",
                json!({"projectId":id,"base":"idle","sceneOnly":true}),
            )?;
            let proof_path = app
                .state::<Shared>()
                .lock()
                .unwrap()
                .root
                .join("decoration-native-proof.json");
            control.eval(r#"setTimeout(async()=>{
              const payload={};
              try {
                document.querySelector('[data-studio-tab=decorations]').click();
                await new Promise(r=>setTimeout(r,800));
                const stage=document.querySelector('[data-preview-surface]'),box=stage.querySelector('.pet-model'),c=box.querySelector('canvas');
                const b=box.getBoundingClientRect(),v=c.getBoundingClientRect(),ctx=c.getContext('2d');
                const pixel=(x,y)=>Array.from(ctx.getImageData(Math.floor((b.x+b.width*x-v.x)/v.width*c.width),Math.floor((b.y+b.height*y-v.y)/v.height*c.height),1,1).data).slice(0,3);
                payload.center=pixel(.5,.5);payload.middle=pixel(.69,.5);payload.behind=pixel(-.1,.5);
                payload.canvasCount=box.querySelectorAll('canvas').length;payload.layers=c.dataset.decorations;
                payload.editorRows=document.querySelectorAll('.decoration-row').length;
                payload.passed=JSON.stringify(payload.center)==='[255,255,0]'&&JSON.stringify(payload.middle)==='[255,0,0]'&&JSON.stringify(payload.behind)==='[0,0,255]'&&payload.canvasCount===1&&payload.editorRows===3;
                // Exercise the actual editor, host validation and late image decode.
                document.querySelector('.decoration-row[data-layer-id=blue] .decoration-select').click();
                const source=Array.from(document.querySelectorAll('.decoration-form label')).find(l=>l.firstChild?.textContent==='图片素材').querySelector('select');
                source.value='green';source.dispatchEvent(new Event('change'));
                const until=performance.now()+6000;
                while(performance.now()<until){await new Promise(r=>setTimeout(r,100));if(JSON.stringify(pixel(-.1,.5))==='[0,255,0]')break;}
                payload.liveSource=pixel(-.1,.5);payload.passed&&=JSON.stringify(payload.liveSource)==='[0,255,0]';
                const pause=ms=>new Promise(r=>setTimeout(r,ms));
                const waitUntil=async predicate=>{const start=performance.now();while(!predicate()){if(performance.now()-start>6000)throw Error('clip creator UI timeout');await pause(80);}};
                document.querySelector('[data-studio-tab=model]').click();
                const open=async()=>{document.querySelector('[data-studio=add-clip]').click();await waitUntil(()=>document.querySelector('.clip-creator'));return document.querySelector('.clip-creator');};
                const set=(dialog,name,value)=>{const el=dialog.querySelector(`[name=${name}]`);el.value=value;el.dispatchEvent(new Event(el.tagName==='SELECT'?'change':'input',{bubbles:true}));};
                let dialog=await open();set(dialog,'asset','blue');await waitUntil(()=>dialog.querySelector('[data-image-info]').textContent.includes('8 × 8'));set(dialog,'id','proof.static');dialog.querySelector('[data-create]').click();await waitUntil(()=>!document.querySelector('.clip-creator'));
                payload.staticClipSelected=document.querySelector('[data-clip]').value==='proof.static';
                dialog=await open();set(dialog,'asset','blue');await waitUntil(()=>dialog.querySelector('[data-image-info]').textContent.includes('8 × 8'));set(dialog,'id','proof.grid');set(dialog,'kind','sequence');set(dialog,'columns','2');set(dialog,'rows','2');dialog.querySelector('[data-infer=counts]').click();
                payload.gridInferred=dialog.querySelector('[name=cellWidth]').value==='4'&&dialog.querySelector('[name=cellHeight]').value==='4'&&dialog.querySelector('[name=frameCount]').value==='4';
                set(dialog,'frameCount','5');dialog.querySelector('[data-create]').click();await pause(150);payload.invalidRejected=!!document.querySelector('.clip-creator')&&dialog.querySelector('[data-feedback]').textContent.includes('容量');
                set(dialog,'frameCount','4');set(dialog,'frameDurationMs','40');set(dialog,'direction','ping-pong');set(dialog,'repeatEndpoints','false');set(dialog,'returnToStart','true');dialog.querySelector('[data-create]').click();await waitUntil(()=>!document.querySelector('.clip-creator'));
                payload.gridClipSelected=document.querySelector('[data-clip]').value==='proof.grid';
                // The new clip must also be selectable as a decoration, without a restart.
                document.querySelector('[data-studio-tab=decorations]').click();
                document.querySelector('.decoration-row[data-layer-id=yellow] .decoration-select').click();
                const labels=()=>Array.from(document.querySelectorAll('.decoration-form label'));
                const kind=labels().find(l=>l.firstChild?.textContent==='内容来源').querySelector('select');kind.value='clip';kind.dispatchEvent(new Event('change'));
                payload.availableToDecoration=Array.from(labels().find(l=>l.firstChild?.textContent==='动画片段').querySelector('select').options).some(o=>o.value==='proof.grid');
                // Save the draft through the UI; Rust checks the persisted fields below.
                document.querySelector('[data-studio=save]').click();await waitUntil(()=>document.querySelector('[data-project-status]').textContent.includes('已保存'));
                payload.passed&&=payload.staticClipSelected&&payload.gridInferred&&payload.invalidRejected&&payload.gridClipSelected&&payload.availableToDecoration;
              } catch(e){payload.error=String(e);payload.passed=false;}
              await window.__TAURI_INTERNALS__.invoke('studio_command',{action:'smoke-decoration-proof',payload});
            },1200)"#).map_err(|e|e.to_string())?;
            let proof_at = Instant::now();
            while !proof_path.exists() {
                std::thread::sleep(Duration::from_millis(100));
                if proof_at.elapsed() > Duration::from_secs(30) {
                    return Err("装饰原生绘制证据超时".into());
                }
            }
            let decoration_proof: Value =
                serde_json::from_slice(&fs::read(proof_path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if decoration_proof["passed"] != true {
                return Err(format!("装饰原生绘制不符：{decoration_proof}"));
            }
            {
                let s = shared.lock().unwrap();
                let p = &s.working(&id)?.project;
                let clips = &p.document.model["clips"];
                if clips["proof.static"]["layout"]["type"] != "single"
                    || clips["proof.grid"]["layout"]["frameCount"] != 4
                    || clips["proof.grid"]["repeatEndpoints"] != false
                    || clips["proof.grid"]["returnToStart"] != true
                    || clips["proof.grid"].get("startReverse").is_some()
                {
                    return Err("片段添加或可选字段保存结果不符".into());
                }
            }
            let clip_task = call("preview", json!({"projectId":id,"clip":"proof.grid"}))?;
            let clip_playback = wait(&shared, clip_task["taskId"].as_u64().unwrap())?;
            let mut scene_cases = vec![];
            for tab in [
                "identity",
                "assets",
                "model",
                "decorations",
                "presets",
                "theme",
            ] {
                scene_cases.push(scene_from_ui(
                    &control,
                    &shared,
                    &id,
                    tab,
                    (tab == "identity").then_some("整体预览保存验证"),
                )?);
            }
            // Force task/generation reuse across two scenes at the same saved
            // revision, then deliver the first scene's late native receipts.
            call("preview", json!({"projectId":id,"sceneOnly":true}))?;
            let old = call("preview", json!({"projectId":id,"clip":"wave"}))?;
            let (old_session, revision, old_generation) = {
                let s = shared.lock().unwrap();
                let t = s
                    .preview
                    .tasks
                    .iter()
                    .find(|t| Some(t.id) == old["taskId"].as_u64())
                    .ok_or("旧场景任务不存在")?;
                (s.preview_session, s.preview_revision, t.generation)
            };
            call("preview", json!({"projectId":id,"sceneOnly":true}))?;
            let current = call("preview", json!({"projectId":id,"clip":"wave"}))?;
            let current_id = current["taskId"].as_u64().ok_or("新场景任务不存在")?;
            let (current_session, current_generation) = {
                let s = shared.lock().unwrap();
                let t = s
                    .preview
                    .tasks
                    .iter()
                    .find(|t| t.id == current_id)
                    .ok_or("新场景任务不存在")?;
                if old["taskId"] != current_id
                    || t.generation != old_generation
                    || s.preview_revision != revision
                    || s.preview_session == old_session
                    || t.status.terminal()
                {
                    return Err("未建立同修订、同任务编号、同 generation 的跨场景回执用例".into());
                }
                (s.preview_session, t.generation)
            };
            for event in ["started", "completed", "failed"] {
                if studio_ack(
                    control.clone(),
                    app.state::<StudioShared>(),
                    id.clone(),
                    revision,
                    old_session,
                    current_id,
                    current_generation,
                    "animation".into(),
                    event.into(),
                )? {
                    return Err(format!("新场景错误地接收旧 session 的 {event} 回执"));
                }
            }
            scene_cases.push(json!({"name":"late native receipts reject old session despite reused task identity","oldSession":old_session,"session":current_session,"task":wait(&shared,current_id)?}));
            // A plain preview after Stop must rebuild its session, without
            // requiring another explicit whole-scene preview first.
            call("stop", json!({"projectId":id}))?;
            let stopped_session = shared.lock().unwrap().preview_session;
            let resumed = call("preview", json!({"projectId":id,"clip":"wave"}))?;
            let resumed_id = resumed["taskId"]
                .as_u64()
                .ok_or("停止后未创建普通预览任务")?;
            {
                let s = shared.lock().unwrap();
                if s.preview_session <= stopped_session
                    || s.preview.current_instance
                        != format!("studio:{id}:{revision}:{}", s.preview_session)
                    || s.preview_model.is_null()
                {
                    return Err("停止后普通预览未重建有效 session".into());
                }
            }
            scene_cases.push(json!({"name":"ordinary animation preview after stop completes with native receipts","task":wait(&shared,resumed_id)?}));
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
            for tab in ["theme", "assets"] {
                scene_cases.push(scene_from_ui(&control, &shared, &theme, tab, None)?);
            }
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
            for tab in ["identity", "rules", "assets"] {
                scene_cases.push(scene_from_ui(&control, &shared, &plugin, tab, None)?);
            }
            // The scene ignores unmatched event conditions, but an explicit
            // simulation must still reject them without replacing that scene.
            let before = shared.lock().unwrap().preview_session;
            let unmatched = call(
                "preview",
                json!({"projectId":plugin,"wall":wall,"event":"pointer.single"}),
            );
            let s = shared.lock().unwrap();
            if !unmatched.is_err_and(|e| e.contains("模拟条件未触发回复"))
                || s.preview_session != before
                || !s.preview.tasks.is_empty()
                || s.preview_model.is_null()
            {
                return Err("显式插件模拟未拒绝不匹配事件，或破坏了整体预览".into());
            }
            drop(s);
            scene_cases.push(json!({"name":"explicit unmatched plugin event preserves scene"}));
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
                json!({"result":"passed","cases":[{"name":"ordered channels and reading","task":first},{"name":"media base-only preview","task":base},{"name":"theme stand-in preview","task":second},{"name":"daily plugin preview","task":third},{"name":"export and reopen","project":reopened},{"name":"runtime isolation","activeInstance":h.store.active},{"name":"created clip native playback","task":clip_playback}],"stateRoutingCases":routed_cases,"scenePreviewCases":scene_cases,"decorationProof":decoration_proof}),
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
fn scene_from_ui(
    control: &WebviewWindow,
    shared: &StudioShared,
    project: &str,
    tab: &str,
    edit_name: Option<&str>,
) -> Result<Value, String> {
    let (before_session, before_revision, kind) = {
        let s = shared.lock().unwrap();
        let p = &s.working(project)?.project.document;
        (s.preview_session, p.revision, p.kind.clone())
    };
    // Use the shared visible button in every authoring tab. Clearing the date
    // catches accidental dependencies on the unrelated simulation form.
    control
        .eval(&format!(
            r#"(async()=>{{
              const panel=document.querySelector('[data-panel=studio]');
              const until=performance.now()+6500;
              while(panel.querySelector('[data-projects]').value!=={project}||panel.hasAttribute('aria-busy')){{
                if(performance.now()>until)return;
                await new Promise(r=>setTimeout(r,60));
              }}
              panel.querySelector('[data-studio-tab="'+{tab}+'"]').click();
              const button=panel.querySelector('[data-preview-card] [data-studio=preview-scene]');
              if(!button||button.hidden||button.closest('[hidden]')||!button.getClientRects().length)return;
              const name={edit_name};
              if(name!==null){{
                const label=Array.from(panel.querySelectorAll('[data-edit-content] label')).find(l=>l.firstChild?.textContent==='显示名称');
                label.querySelector('input').value=name;
                label.querySelector('input').dispatchEvent(new Event('input',{{bubbles:true}}));
                if(!panel.querySelector('[data-project-status]').textContent.includes('有未保存编辑'))return;
              }}
              panel.querySelector('[data-virtual-time]').value='';
              panel.querySelector('[data-preview-base]').value='idle';
              panel.querySelector('[data-event]').value='pointer.single';
              button.click();
            }})();"#,
            project = serde_json::to_string(project).unwrap(),
            tab = serde_json::to_string(tab).unwrap(),
            edit_name = serde_json::to_string(&edit_name).unwrap(),
        ))
        .map_err(|e| e.to_string())?;
    let at = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(80));
        let s = shared.lock().unwrap();
        let working = s.working(project)?;
        if s.preview_session > before_session
            && s.preview_project == project
            && s.preview_revision == working.project.document.revision
            && !s.preview_model.is_null()
        {
            if !s.preview.tasks.is_empty() || s.preview.active().is_some() {
                return Err(format!("{kind}/{tab} 整体预览错误地产生了回复任务"));
            }
            if let Some(name) = edit_name {
                let persisted: Value = serde_json::from_slice(
                    &fs::read(working.root.join("project.json")).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if working.project.document.revision <= before_revision
                    || working.project.document.manifest["displayName"] != name
                    || persisted["document"]["manifest"]["displayName"] != name
                    || persisted["document"]["revision"] != working.project.document.revision
                {
                    return Err("整体预览未先保存界面上的未保存编辑".into());
                }
            }
            return Ok(
                json!({"name":format!("UI scene {kind}/{tab}"),"emptySimulationDate":true,"unmatchedEvent":"pointer.single","savedDraft":edit_name.is_some(),"session":s.preview_session,"finiteTasks":s.preview.tasks.len()}),
            );
        }
        if at.elapsed() > Duration::from_secs(9) {
            return Err(format!(
                "{kind}/{tab} 的可见整体预览按钮未启动场景（模拟日期为空）"
            ));
        }
    }
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
