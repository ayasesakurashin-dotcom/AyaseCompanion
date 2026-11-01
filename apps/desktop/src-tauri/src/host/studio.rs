#[cfg(feature = "smoke-test")]
pub(super) mod smoke;
use super::*;
use ayase_core::{
    scheduler::Status,
    studio::{Document, Project},
    theme,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
pub(super) type StudioShared = Arc<Mutex<Studio>>;
struct Working {
    root: PathBuf,
    project: Project,
}
struct Prepared {
    project: String,
    revision: u64,
    id: u64,
    generation: u64,
    result: Result<(Value, Vec<String>), String>,
}
pub(super) struct Studio {
    root: PathBuf,
    active: Option<String>,
    projects: BTreeMap<String, Working>,
    plugins: BTreeMap<String, Plugin>,
    preview: Scheduler,
    preview_project: String,
    preview_revision: u64,
    preview_model: Value,
    preview_paths: Value,
    preview_theme: Value,
    preview_theme_paths: Value,
    preview_zoom: f64,
    origin: Instant,
    cancel: Arc<AtomicBool>,
    prepared: Option<Receiver<Prepared>>,
    notice: String,
    recent: BTreeMap<String, String>,
    preview_package: Option<Package>,
    preview_root: PathBuf,
}
pub(super) fn new(root: PathBuf) -> Result<StudioShared, Box<dyn std::error::Error>> {
    fs::create_dir_all(&root)?;
    let library = root.join("plugin-library");
    fs::create_dir_all(&library)?;
    let mut imported = BTreeMap::new();
    let index: BTreeMap<String, String> = fs::read(library.join("index.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    for (expected, hash) in index {
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if let Ok(b) = fs::read(library.join(format!("{hash}.ayplug"))) {
            if let Ok(p) = package::validate(&b) {
                if p.report.kind == "plugin" && p.report.id == expected && p.report.hash == hash {
                    if let Ok(plugin) = Plugin::from_files(p.manifest, &p.files) {
                        imported.insert(p.report.id, plugin);
                    }
                }
            }
        }
    }
    Ok(Arc::new(Mutex::new(Studio {
        root,
        active: None,
        projects: BTreeMap::new(),
        plugins: imported,
        preview: Scheduler::new("studio"),
        preview_project: String::new(),
        preview_revision: 0,
        preview_model: Value::Null,
        preview_paths: json!({}),
        preview_theme: theme::defaults(),
        preview_theme_paths: json!({}),
        preview_zoom: 1.0,
        origin: Instant::now(),
        cancel: Arc::new(AtomicBool::new(false)),
        prepared: None,
        notice: String::new(),
        recent: BTreeMap::new(),
        preview_package: None,
        preview_root: PathBuf::new(),
    })))
}
fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&temp, path).map_err(|e| e.to_string())
}
fn persist(root: &std::path::Path, p: &Project) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut hashes = serde_json::Map::new();
    for (path, b) in &p.files {
        package::safe_path(path)?;
        let target = root.join("files").join(path);
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        let blob = root.join("blobs").join(package::digest(b));
        fs::create_dir_all(blob.parent().unwrap()).map_err(|e| e.to_string())?;
        if !blob.exists() {
            atomic_write(&blob, b)?;
        }
        atomic_write(&target, b)?;
        hashes.insert(path.clone(), json!(package::digest(b)));
    }
    let bytes=serde_json::to_vec_pretty(&json!({"format":"ayase.workspace","formatVersion":"1.0","document":p.document,"files":hashes})).unwrap();
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("项目文档超过2MiB".into());
    }
    atomic_write(&root.join("project.json"), &bytes)
}
fn open_workspace(path: &std::path::Path) -> Result<Project, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > 2 * 1024 * 1024 {
        return Err("项目文档过大".into());
    }
    let value: Value = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if value["format"] != "ayase.workspace" || value["formatVersion"] != "1.0" {
        return Err("不是支持的制作项目".into());
    }
    let doc: Document =
        serde_json::from_value(value["document"].clone()).map_err(|e| e.to_string())?;
    if doc.format != "ayase.project"
        || doc.format_version != "1.0"
        || !["character", "plugin", "theme"].contains(&doc.kind.as_str())
    {
        return Err("项目类型无效".into());
    }
    package::no_private_fields(&value["document"])?;
    let rows = value["files"].as_object().ok_or("项目缺少文件清单")?;
    if rows.len() > 2048 {
        return Err("项目文件过多".into());
    }
    let mut files = BTreeMap::new();
    let mut total = 0;
    let base = path.parent().unwrap().join("files");
    let canonical_root = fs::canonicalize(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let mut seen = std::collections::BTreeSet::new();
    for (name, hash) in rows {
        if !seen.insert(package::safe_path(name)?) {
            return Err("项目文件规范化重名".into());
        }
        let mut checked = base.clone();
        for part in name.split('/') {
            checked.push(part);
            let m = fs::symlink_metadata(&checked).map_err(|e| e.to_string())?;
            if m.file_type().is_symlink() {
                return Err("项目不读取符号链接".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if m.file_attributes() & 0x400 != 0 {
                    return Err("项目不读取重解析路径".into());
                }
            }
        }
        let size = fs::metadata(&checked).map_err(|e| e.to_string())?.len();
        if !fs::canonicalize(&checked)
            .map_err(|e| e.to_string())?
            .starts_with(&canonical_root)
        {
            return Err("项目素材不能离开项目目录".into());
        }
        total += size;
        if size > package::MAX_FILE || total > package::MAX_TOTAL {
            return Err("项目文件预算超限".into());
        }
        let mut b = fs::read(&checked).map_err(|e| e.to_string())?;
        if hash.as_str() != Some(&package::digest(&b)) {
            if let Some(h) = hash
                .as_str()
                .filter(|h| h.len() == 64 && h.bytes().all(|c| c.is_ascii_hexdigit()))
            {
                let blob = path.parent().unwrap().join("blobs").join(h);
                if let Ok(saved) = fs::read(blob) {
                    b = saved;
                }
            }
        }
        if hash.as_str() != Some(&package::digest(&b)) {
            return Err(format!("项目素材散列不符：{name}"));
        }
        files.insert(name.clone(), b);
    }
    Ok(Project {
        document: doc,
        files,
    })
}
impl Studio {
    fn now(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }
    fn stop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.preview.stop(self.now());
        self.prepared = None;
    }
    fn insert(&mut self, project: Project) -> Result<String, String> {
        let id = Uuid::new_v4().to_string();
        let root = self.root.join(&id);
        persist(&root, &project)?;
        self.stop();
        self.projects.insert(id.clone(), Working { root, project });
        self.active = Some(id.clone());
        Ok(id)
    }
    fn working(&self, id: &str) -> Result<&Working, String> {
        if self.active.as_deref() != Some(id) {
            return Err("制作项目已切换".into());
        }
        self.projects.get(id).ok_or("项目未载入".into())
    }
    fn commit_project(&mut self, id: &str, p: Project) -> Result<(), String> {
        let root = self.working(id)?.root.clone();
        persist(&root, &p)?;
        self.projects.get_mut(id).unwrap().project = p;
        self.stop();
        Ok(())
    }
    fn tick(&mut self) {
        if let Some(rx) = &self.prepared {
            if let Ok(done) = rx.try_recv() {
                let valid = self.active.as_deref() == Some(&done.project)
                    && self
                        .working(&done.project)
                        .is_ok_and(|w| w.project.document.revision == done.revision);
                if valid {
                    match done.result {
                        Ok((mut plan, channels)) => {
                            if let Some(task) = self.preview.tasks.iter().find(|t| t.id == done.id)
                            {
                                plan["stateGuard"] = task.plan["stateGuard"].clone();
                            }
                            self.preview.ready(
                                done.id,
                                done.generation,
                                plan,
                                channels,
                                self.now(),
                            );
                        }
                        Err(e) => {
                            self.notice = e.clone();
                            self.preview.finish(done.id, Status::Failed, &e, self.now());
                        }
                    }
                }
                self.prepared = None;
            }
        }
        self.preview.reconcile_base(&self.preview_model, self.now());
        self.preview.tick(self.now());
        // Choose against the last *started* variant after the scheduler grants
        // the next slot. Cancelled queued previews do not advance this history.
        if let Some(t) = self
            .preview
            .active()
            .filter(|t| {
                t.status == Status::Starting
                    && t.plan["studioChoice"].is_object()
                    && t.plan["studioChosen"].is_null()
            })
            .cloned()
        {
            let choice = t.plan["studioChoice"].clone();
            let key = choice["key"].as_str().unwrap_or("");
            let mut response = choice["response"].clone();
            if let Some(last) = self.recent.get(key) {
                let variants = response["variants"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|v| v["id"] != *last)
                    .cloned()
                    .collect::<Vec<_>>();
                if !variants.is_empty() {
                    response["variants"] = json!(variants);
                }
            }
            let result = plugin::choose_variant(&response, 0.5)
                .ok_or("没有固定回复".to_owned())
                .and_then(|v| {
                    ayase_core::presentation::resolve(
                        self.preview_package.as_ref().unwrap(),
                        &json!({}),
                        false,
                        &self.preview_root,
                        &v,
                        "studio.preview",
                    )
                    .map(|(mut p, c)| {
                        p["studioChoice"] = choice;
                        p["studioChosen"] = v["id"].clone();
                        p["stateGuard"] = t.plan["stateGuard"].clone();
                        (p, c)
                    })
                });
            match result {
                Ok((plan, channels)) => {
                    let task = self
                        .preview
                        .tasks
                        .iter_mut()
                        .find(|v| v.id == t.id)
                        .unwrap();
                    task.plan = plan;
                    task.channels = channels
                        .into_iter()
                        .map(|c| (c, "pending".into()))
                        .collect();
                }
                Err(e) => {
                    self.notice = e.clone();
                    self.preview.finish(t.id, Status::Failed, &e, self.now());
                }
            }
        }
    }
    fn snapshot(&mut self, registry: &BTreeMap<String, Plugin>) -> Value {
        self.tick();
        let projects=fs::read_dir(&self.root).ok().into_iter().flatten().flatten().filter_map(|d|{let path=d.path().join("project.json");let v:Value=serde_json::from_slice(&fs::read(path).ok()?).ok()?;Some(json!({"id":d.file_name().to_string_lossy(),"kind":v["document"]["kind"],"name":v["document"]["manifest"]["displayName"]}))}).collect::<Vec<_>>();
        let current=self.active.as_ref().and_then(|id|self.projects.get(id).map(|w|json!({"id":id,"document":w.project.document,"assets":w.project.document.manifest["assets"],"assetPaths":w.project.document.manifest["assets"].as_object().map(|a|a.iter().filter_map(|(id,v)|Some((id.clone(),json!(w.root.join("files").join(v["path"].as_str()?).to_string_lossy())))).collect::<serde_json::Map<_,_>>()),"files":w.project.files.iter().map(|(p,b)|json!({"path":p,"bytes":b.len()})).collect::<Vec<_>>()})));
        let mut plugins = registry.clone();
        plugins.extend(self.plugins.clone());
        json!({"projects":projects,"current":current,"plugins":plugins.iter().map(|(id,p)|json!({"id":id,"name":p.manifest["displayName"],"schema":p.schema,"defaults":p.manifest["defaults"],"configVersion":p.manifest["configVersion"],"priority":p.manifest["defaultPriority"]})).collect::<Vec<_>>(),"contracts":{"sprite":ayase_core::schema::contract("sprite"),"request":ayase_core::schema::contract("request"),"theme":ayase_core::schema::contract("theme"),"rules":ayase_core::schema::contract("rules")},"preview":{"projectId":self.preview_project,"revision":self.preview_revision,"model":self.preview_model,"assetPaths":self.preview_paths,"theme":self.preview_theme,"themeAssetPaths":self.preview_theme_paths,"zoom":self.preview_zoom,"scheduler":self.preview.snapshot()},"notice":self.notice})
    }
}
#[tauri::command]
pub(super) fn studio_snapshot(
    window: WebviewWindow,
    state: State<Shared>,
    studio: State<StudioShared>,
) -> Result<Value, String> {
    control(&window)?;
    let plugins = state.lock().unwrap().plugins.clone();
    let mut s = studio.lock().unwrap();
    if let Some(w) = s.active.as_ref().and_then(|id| s.projects.get(id)) {
        let _ = window
            .app_handle()
            .asset_protocol_scope()
            .allow_directory(w.root.join("files"), true);
    }
    Ok(s.snapshot(&plugins))
}
#[tauri::command]
pub(super) fn studio_ack(
    window: WebviewWindow,
    studio: State<StudioShared>,
    project_id: String,
    revision: u64,
    id: u64,
    generation: u64,
    channel: String,
    event: String,
) -> Result<bool, String> {
    control(&window)?;
    let mut s = studio.lock().unwrap();
    if s.active.as_deref() != Some(&project_id)
        || s.preview_project != project_id
        || s.preview_revision != revision
    {
        return Ok(false);
    }
    let now = s.now();
    Ok(match event.as_str() {
        "base-started" => s
            .preview
            .base_transition_ack(id, generation, "started", now),
        "base-completed" => s
            .preview
            .base_transition_ack(id, generation, "completed", now),
        "base-failed" => s.preview.base_transition_ack(id, generation, "failed", now),
        "started" => {
            let base = s
                .preview
                .claims
                .get("studio.environment")
                .map(|c| c.mode.as_str())
                .unwrap_or("idle");
            let valid = s
                .preview
                .tasks
                .iter()
                .find(|t| t.id == id && t.generation == generation)
                .is_some_and(|t| {
                    !t.plan["stateGuard"].is_object() || t.plan["stateGuard"]["base"] == base
                });
            if valid {
                let started = s.preview.started(id, generation, now);
                if started {
                    if let Some(t) = s.preview.tasks.iter().find(|t| t.id == id).cloned() {
                        if let (Some(key), Some(v)) = (
                            t.plan["studioChoice"]["key"].as_str(),
                            t.plan["studioChosen"].as_str(),
                        ) {
                            s.recent.insert(key.into(), v.into());
                        }
                    }
                }
                started
            } else {
                s.preview
                    .finish(id, Status::Cancelled, "模拟基础状态已改变", now);
                false
            }
        }
        "completed" => s.preview.channel(id, generation, &channel, false, now),
        "failed" => s.preview.channel(id, generation, &channel, true, now),
        _ => false,
    })
}
fn write_new(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        return Err("请选择新的文件名，保留旧版本".into());
    }
    atomic_write(path, bytes)
}
#[tauri::command]
pub(super) async fn studio_command(
    window: WebviewWindow,
    state: State<'_, Shared>,
    studio: State<'_, StudioShared>,
    action: String,
    payload: Value,
) -> Result<Value, String> {
    control(&window)?;
    let shared = studio.inner().clone();
    let app = window.app_handle().clone();
    let (registry, settings, profile, chat_key, voice_key, current, default_theme) = {
        let h = state.lock().unwrap();
        let real = action == "preview" && payload["realServices"] == true;
        let keys = if real {
            (
                h.ai.vault.key("chat", &h.ai.settings.chat.endpoint)?,
                h.ai.vault.key("voice", &h.ai.settings.voice.endpoint)?,
            )
        } else {
            (String::new(), String::new())
        };
        let current = if action == "from-current" {
            let mut p = h.package.clone().ok_or("没有当前伙伴")?;
            p.manifest["pluginPresets"] = json!(h
                .presets()
                .into_iter()
                .map(|mut v| {
                    if let Some(c) = h.configured().iter().find(|c| v["presetId"] == c.owner) {
                        v["config"] = c.config.clone();
                        v["configMode"] = json!("complete");
                        v["enabled"] = json!(c.enabled);
                        v["priority"] = json!(c.priority);
                    }
                    v
                })
                .collect::<Vec<_>>());
            materialize_identity_assets(&mut p.manifest, &h.instance().unwrap().overrides);
            Some(Project::from_package(p)?)
        } else {
            None
        };
        (
            h.plugins.clone(),
            h.ai.settings.clone(),
            h.profile(),
            keys.0,
            keys.1,
            current,
            json!({"settings":h.store.appearance,"assetPaths":h.appearance_assets}),
        )
    };
    tauri::async_runtime::spawn_blocking(move||{
  let mut s=shared.lock().unwrap();s.tick();let id=payload["projectId"].as_str().unwrap_or("").to_owned();
  match action.as_str(){
   "from-current"=>{let id=s.insert(current.ok_or("没有当前伙伴")?)?;Ok(json!({"projectId":id}))},
   "new"=>{let p=match payload["kind"].as_str(){Some("plugin")=>Project::rule_plugin(),Some("theme")=>Project::theme(),_=>Project::character()};let id=s.insert(p)?;Ok(json!({"projectId":id}))},
   "open-package"=>{let path=payload["path"].as_str().ok_or("请选择包")?;if fs::metadata(path).map_err(|e|e.to_string())?.len()>package::MAX_TOTAL{return Err("包过大".into());}let p=package::validate(&fs::read(path).map_err(|e|e.to_string())?)?;let id=s.insert(Project::from_package(p)?)?;Ok(json!({"projectId":id}))},
   "open-workspace"=>{let p=open_workspace(std::path::Path::new(payload["path"].as_str().ok_or("请选择project.json")?))?;let id=s.insert(p)?;Ok(json!({"projectId":id}))},
   "select"=>{if !s.projects.contains_key(&id){if id.len()!=36||id.contains(['/', '\\']){return Err("项目编号无效".into());}let root=s.root.join(&id);let p=open_workspace(&root.join("project.json"))?;s.projects.insert(id.clone(),Working{root,project:p});}s.stop();s.active=Some(id);Ok(json!(true))},
   "save"=>{let mut p=s.working(&id)?.project.clone();let doc:Document=serde_json::from_value(payload["document"].clone()).map_err(|e|e.to_string())?;p.save_document(payload["revision"].as_u64().ok_or("缺少项目修订")?,doc)?;s.commit_project(&id,p)?;Ok(json!(true))},
   "save-workspace"=>{let p=s.working(&id)?.project.clone();let target=PathBuf::from(payload["path"].as_str().ok_or("请选择新的项目文件夹")?);if target.exists(){return Err("请选择尚不存在的项目文件夹，避免覆盖已有项目".into());}persist(&target,&p)?;Ok(json!({"path":target.join("project.json").to_string_lossy()}))},
   "add-assets"=>{let mut p=s.working(&id)?.project.clone();for item in payload["items"].as_array().ok_or("缺少素材")?{let source=item["path"].as_str().ok_or("素材缺少路径")?;let source_path=std::path::Path::new(source);let ext=source_path.extension().and_then(|s|s.to_str()).unwrap_or("").to_ascii_lowercase();let kind=item["kind"].as_str().unwrap_or(match ext.as_str(){"png"|"webp"=>"image","txt"=>"text","wav"|"mp3"|"ogg"|"flac"=>"audio",_=>"data"});if fs::metadata(source).map_err(|e|e.to_string())?.len()>package::MAX_FILE{return Err("素材过大".into());}let name=item["id"].as_str().map(str::to_owned).unwrap_or_else(||source_path.file_stem().unwrap_or_default().to_string_lossy().into());let path=if let Some(previous)=p.document.manifest["assets"][&name]["path"].as_str(){previous.to_owned()}else{format!("assets/{}.{}",Uuid::new_v4(),ext)};p.asset(&name,kind,&path,fs::read(source).map_err(|e|e.to_string())?)?;}
    s.commit_project(&id,p)?;let root=s.working(&id)?.root.clone();let _=app.asset_protocol_scope().allow_directory(root.join("files"),true);Ok(json!(true))},
   "references"=>Ok(json!(s.working(&id)?.project.references(payload["assetId"].as_str().ok_or("缺少素材编号")?))),
   "rename-asset"|"delete-asset"=>{let mut p=s.working(&id)?.project.clone();let asset=payload["assetId"].as_str().ok_or("缺少素材编号")?;if action=="delete-asset"{p.delete_asset(asset)?;}else{let mut known=registry.keys().cloned().collect::<std::collections::BTreeSet<_>>();known.extend(s.plugins.keys().cloned());p.rename_asset_known(asset,payload["newId"].as_str().ok_or("缺少新编号")?,&known)?;}s.commit_project(&id,p)?;Ok(json!(true))},
   "text"=>{let w=s.working(&id)?;let asset=payload["assetId"].as_str().ok_or("缺少素材编号")?;if w.project.document.manifest["assets"][asset]["kind"]!="text"{return Err("请选择文字素材".into());}let path=w.project.document.manifest["assets"][asset]["path"].as_str().unwrap();let text=String::from_utf8(w.project.files.get(path).ok_or("文字文件缺失")?.clone()).map_err(|e|e.to_string())?;Ok(json!(text))},
   "save-text"=>{let mut p=s.working(&id)?.project.clone();let asset=payload["assetId"].as_str().ok_or("缺少素材编号")?;let path=p.document.manifest["assets"][asset]["path"].as_str().ok_or("素材不存在")?.to_owned();p.asset(asset,"text",&path,payload["text"].as_str().ok_or("文字无效")?.as_bytes().to_vec())?;s.commit_project(&id,p)?;Ok(json!(true))},
   "import-clips"=>{let path=payload["path"].as_str().ok_or("请选择动作清单")?;if fs::metadata(path).map_err(|e|e.to_string())?.len()>2*1024*1024{return Err("清单过大".into());}let value:Value=serde_json::from_slice(&fs::read(path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;let mut p=s.working(&id)?.project.clone();ayase_core::schema::validate(&ayase_core::schema::contract("sprite"),&value)?;p.document.model=value;p.document.revision+=1;s.commit_project(&id,p)?;Ok(json!(true))},
   "import-plugin"=>{let path=payload["path"].as_str().ok_or("请选择插件包")?;if fs::metadata(path).map_err(|e|e.to_string())?.len()>package::MAX_TOTAL{return Err("插件包过大".into());}let p=package::validate(&fs::read(path).map_err(|e|e.to_string())?)?;if p.report.kind!="plugin"{return Err("请选择规则插件".into());}let plugin=Plugin::from_files(p.manifest,&p.files)?;let dest=s.root.join("plugin-library").join(format!("{}.ayplug",p.report.hash));if !dest.exists(){write_new(&dest,&fs::read(path).map_err(|e|e.to_string())?)?;}let index_path=s.root.join("plugin-library/index.json");let mut index:BTreeMap<String,String>=fs::read(&index_path).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_default();index.insert(p.report.id.clone(),p.report.hash);atomic_write(&index_path,&serde_json::to_vec_pretty(&index).unwrap())?;s.plugins.insert(p.report.id,plugin);Ok(json!(true))},
   "import-presets"=>{let path=payload["path"].as_str().ok_or("请选择预设清单")?;if fs::metadata(path).map_err(|e|e.to_string())?.len()>2*1024*1024{return Err("预设清单过大".into());}let rows:Value=serde_json::from_slice(&fs::read(path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;package::no_private_fields(&rows)?;let mut p=s.working(&id)?.project.clone();if p.document.kind!="character"{return Err("请打开角色项目".into());}let list=p.document.manifest["pluginPresets"].as_array_mut().ok_or("缺少预设数组")?;for row in rows.as_array().ok_or("预设清单必须是数组")?{let owner=row["presetId"].as_str().ok_or("预设缺少presetId")?;if let Some(index)=list.iter().position(|p|p["presetId"]==owner){list[index]=row.clone();}else{list.push(row.clone());}}ayase_core::schema::validate(&ayase_core::schema::contract("character"),&p.document.manifest)?;p.document.revision+=1;s.commit_project(&id,p)?;Ok(json!(true))},
   "validate"|"export"=>{let mut p=s.working(&id)?.project.clone();let mut plugins=registry.clone();plugins.extend(s.plugins.clone());if p.document.kind=="character"{p.document.manifest["pluginPresets"]=ayase_core::studio::effective_presets(&p.document.manifest,&plugins)?;}let bytes=p.export()?;let package=package::validate(&bytes)?;if action=="export"{write_new(std::path::Path::new(payload["path"].as_str().ok_or("请选择新导出文件")?),&bytes)?;}Ok(json!({"report":package.report,"files":package.files.iter().map(|(path,b)|json!({"path":path,"bytes":b.len(),"sha256":package::digest(b)})).collect::<Vec<_>>()}))},
   "stop"=>{s.stop();Ok(json!(true))},
   "preview"=>preview(&mut s,&app,&registry,settings,profile,chat_key,voice_key,default_theme,&id,&payload),
   _=>Err("未知工作台操作".into())
  }
 }).await.map_err(|e|e.to_string())?
}

fn preview(
    s: &mut Studio,
    app: &tauri::AppHandle,
    registry: &BTreeMap<String, Plugin>,
    mut settings: services::Settings,
    mut profile: conversation::Profile,
    chat_key: String,
    voice_key: String,
    default_theme: Value,
    id: &str,
    options: &Value,
) -> Result<Value, String> {
    let w = s.working(id)?;
    let original = w.project.validated()?;
    let root = w.root.join("files");
    let mut doc = w.project.document.clone();
    let mut registry = registry.clone();
    let package = if original.report.kind == "character" {
        original
    } else {
        let mut standin = Project::character();
        standin.files = original.files.clone();
        standin.files.remove("theme.json");
        standin.files.remove("plugin.json");
        standin.document.manifest["assets"] = original.manifest["assets"]
            .as_object()
            .map(|a| json!(a))
            .unwrap_or(json!({}));
        let bytes = include_bytes!("../../icons/icon.png").to_vec();
        let (width, height) = package::image_dimensions(&bytes)?;
        let icon = root.join("preview/standin.png");
        fs::create_dir_all(icon.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(&icon, &bytes).map_err(|e| e.to_string())?;
        standin.asset("studio.standin", "image", "preview/standin.png", bytes)?;
        standin.document.model["logicalSize"] = json!({"width":width,"height":height});
        standin.document.model["clips"]["idle"] = json!({"asset":"studio.standin","layout":{"type":"single"},"frameDurationMs":1000,"direction":"forward"});
        if original.report.kind == "theme" {
            standin.document.theme = original.manifest.clone();
            doc.theme = original.manifest;
        } else {
            let plugin = Plugin::from_files(original.manifest.clone(), &original.files)?;
            doc.manifest["pluginPresets"] = json!([{"presetId":"studio.plugin","pluginId":original.report.id,"configVersion":"1.0","enabled":true,"priority":"normal","configMode":"complete","config":original.manifest["defaults"]}]);
            registry.insert(original.report.id, plugin);
        }
        standin.validated()?
    };
    let epoch = doc.revision;
    let mut response = options["response"].clone();
    let base = options["base"].as_str().unwrap_or("idle");
    let mut plugins = registry.clone();
    plugins.extend(s.plugins.clone());
    let mut source = "studio.preview".to_owned();
    let mut claimed = None;
    let mut claim_only = false;
    let mut priority = Priority::Normal;
    if let Some(preset_id) = if doc.kind == "plugin" {
        Some("studio.plugin")
    } else {
        options["presetId"].as_str()
    } {
        let preset = doc.manifest["pluginPresets"]
            .as_array()
            .and_then(|a| a.iter().find(|p| p["presetId"] == preset_id))
            .ok_or("配置不存在")?;
        let plugin = plugins
            .get(preset["pluginId"].as_str().unwrap())
            .ok_or("请先在工作台导入对应插件")?;
        let c = plugin.configure(preset, &json!({}), 0);
        if !c.enabled {
            return Err(format!("模拟配置未启用：{}", c.status));
        }
        priority = c.priority;
        let wall: DateTime<Utc> = options["wall"]
            .as_str()
            .unwrap_or("2030-01-01T23:00:00Z")
            .parse()
            .map_err(|_| "模拟时间无效")?;
        let mut runtime = Runtime::default();
        let mut responses = vec![];
        if let Some(kind) = plugin.manifest["hostBehavior"].as_str() {
            let mut b = ayase_core::behavior::Input {
                monotonic_ms: options["elapsedMs"].as_u64().unwrap_or(4_000_000),
                idle_seconds: Some(0),
                pointer: Some((
                    options["click"].as_str().unwrap_or("double").into(),
                    options["area"].as_str().unwrap_or("").into(),
                )),
                manual_work: true,
                ..Default::default()
            };
            b.weather = options["weather"]
                .as_object()
                .map(|_| options["weather"].clone());
            b.calendar = options["calendar"].as_array().cloned().unwrap_or_default();
            b.special = options["special"].as_array().cloned().unwrap_or_default();
            if let Some(candidate) = ayase_core::behavior::simulate(kind, &c, b, wall) {
                responses.push(candidate.response);
            }
        } else {
            for output in plugin.dispatch(
                &c,
                options["event"].as_str().unwrap_or("clock.tick"),
                Some(options["environmentActive"] == true),
                wall,
                &mut runtime,
            ) {
                match output {
                    Output::Present(r, _) => responses.push(r),
                    Output::Claim(mode, suppress) => {
                        claimed = Some((mode, suppress));
                    }
                    _ => {}
                }
            }
        }
        response = if let Some(r) = responses.into_iter().next() {
            r
        } else if claimed.is_some() {
            claim_only = true;
            json!({"mode":"fixed","variants":[{"id":"base-only","text":"姿态预览"}]})
        } else {
            return Err("模拟条件未触发回复，请检查启用、时间、状态与阈值".into());
        };
        source = preset_id.into();
    }
    if let Some(clip) = options["clip"].as_str() {
        response = json!({"mode":"fixed","variants":[{"id":"clip","animationAsset":clip}]});
    }
    if let Some(asset) = options["audioAsset"].as_str() {
        response = json!({"mode":"fixed","variants":[{"id":"audio","audioAsset":asset}]});
    }
    if response.is_null() {
        response = json!({"mode":"fixed","variants":[{"id":"sample","text":options["text"].as_str().unwrap_or("你好。这是工作台预览。\n中文长句、多页、缩放和阅读停留都使用宿主相同的呈现组件。"),"cue":"greeting"}]});
    }
    plugin::validate_response(&response)?;
    let (response, route) =
        plugin::response_for_base(&response, base).ok_or("当前模拟状态没有匹配回复")?;
    let response = response.clone();
    if s.preview_project != id || s.preview_revision != epoch {
        s.stop();
        s.preview = Scheduler::new(&format!("studio:{id}:{epoch}"));
        s.preview_project = id.into();
        s.preview_revision = epoch;
    }
    s.preview
        .claim("studio.environment", base, Priority::Normal, false);
    s.preview.applied_base = base.into();
    s.preview.desired_base = base.into();
    s.preview_model = package.model.clone().unwrap();
    s.preview_paths = json!(package.manifest["assets"]
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(id, a)| Some((
            id.clone(),
            json!(root.join(a["path"].as_str()?).to_string_lossy())
        )))
        .collect::<serde_json::Map<_, _>>());
    s.preview_zoom = options["zoom"].as_f64().unwrap_or(1.0).clamp(0.6, 1.6);
    s.preview_theme_paths = if doc.theme.is_null() {
        default_theme["assetPaths"].clone()
    } else {
        json!(doc.theme["assets"]
            .as_object()
            .map(|a| a
                .iter()
                .filter_map(|(id, v)| Some((
                    id.clone(),
                    json!(root.join(v["path"].as_str()?).to_string_lossy())
                )))
                .collect::<serde_json::Map<_, _>>())
            .unwrap_or_default())
    };
    s.preview_theme = if doc.theme.is_null() {
        default_theme["settings"].clone()
    } else {
        doc.theme.clone()
    };
    let _ = app.asset_protocol_scope().allow_directory(&root, true);
    if claim_only {
        let (mode, suppress) = claimed.unwrap();
        s.preview
            .claim("studio.environment", &mode, priority, suppress);
        s.tick();
        return Ok(json!({"taskId":s.preview.active().map(|t|t.id).unwrap_or(0),"baseOnly":true}));
    }
    let real = options["realServices"] == true && response["mode"] != "fixed";
    let choice = |r: &Value| plugin::choose_variant(r, 0.5).ok_or("固定回复为空");
    let (mut plan, channels) = if response["mode"] == "fixed" {
        let key = format!("{id}:{source}:{}", route.unwrap_or("all"));
        let mut r = response.clone();
        if let Some(last) = s.recent.get(&key) {
            let all = r["variants"].as_array().unwrap().clone();
            let eligible = all
                .iter()
                .filter(|v| v["id"] != *last)
                .cloned()
                .collect::<Vec<_>>();
            if !eligible.is_empty() {
                r["variants"] = json!(eligible);
            }
        }
        let v = choice(&r)?;

        ayase_core::presentation::resolve(&package, &json!({}), false, &root, &v, "studio.preview")?
    } else if real {
        (
            json!({"preparationStatus":"正在使用已配置的真实服务准备预览；不写入伙伴历史"}),
            vec![],
        )
    } else {
        ayase_core::presentation::resolve(
            &package,
            &json!({}),
            false,
            &root,
            &json!({"text":format!("{}",response["intent"].as_str().unwrap_or("自然回应")),"cue":response["cue"],"animationAsset":response["animationAsset"]}),
            "studio.preview",
        )?
    };
    s.preview_package = Some(package.clone());
    s.preview_root = root.clone();
    if response["mode"] == "fixed" {
        plan["studioChoice"] =
            json!({"key":format!("{id}:{source}:{}",route.unwrap_or("all")),"response":response});
    }
    if let Some(route) = route {
        plan["stateGuard"] = json!({"base":base,"routeId":route});
    }
    let task = s.preview.submit(
        Request {
            instance_id: format!("studio:{id}:{epoch}"),
            owner: "studio.preview".into(),
            revision: 0,
            priority,
            proactive: false,
            ttl_ms: 300000,
            preparing: real,
            plan,
            channels,
            dedupe: None,
        },
        s.now(),
    )?;
    if real {
        if s.prepared.is_some() {
            s.preview.finish(
                task,
                Status::Failed,
                "已有真实预览正在准备，请等待或停止",
                s.now(),
            );
            return Err("已有真实预览正在准备".into());
        }
        let prepared_task = s.preview.tasks.iter_mut().find(|t| t.id == task).unwrap();
        prepared_task.prepare_deadline = prepared_task.deadline;
        let generation = prepared_task.generation;
        settings.memory.enabled = false;
        profile.character_name = doc.manifest["identity"]["characterName"]
            .as_str()
            .unwrap_or("伙伴")
            .into();
        profile.persona = doc.manifest["identity"]["persona"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                doc.manifest["identity"]["personaAsset"]
                    .as_str()
                    .and_then(|id| package.asset_text(id))
            })
            .unwrap_or_else(|| "友善、自然地交流。".into());
        profile.user_address = doc.manifest["identity"]["userAddress"]
            .as_str()
            .unwrap_or("你")
            .into();
        profile.sync_memory = false;
        profile.voice_enabled = options["realVoice"] == true;
        let spec = conversation::Spec {
            context: crate::p3::Context::default(),
            id: task,
            generation,
            instance: format!("studio:{id}:{epoch}"),
            epoch: 0,
            profile_revision: 0,
            turn: Uuid::new_v4().to_string(),
            profile,
            input: response["intent"].as_str().unwrap_or("预览回复").into(),
            source: "studio.preview".into(),
            response: response.clone(),
            bank: String::new(),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        s.cancel = cancel.clone();
        let (tx, rx) = mpsc::channel();
        s.prepared = Some(rx);
        let owner = id.to_owned();
        let audio_root = s
            .root
            .join("preview-audio")
            .join(Uuid::new_v4().to_string());
        let _ = app
            .asset_protocol_scope()
            .allow_directory(&audio_root, true);
        std::thread::spawn(move || {
            let rx = conversation::launch(
                spec,
                settings,
                chat_key,
                voice_key,
                String::new(),
                (String::new(), String::new()),
                vec![],
                cancel.clone(),
            );
            let result = (|| {
                let mut text = String::new();
                let mut audio = BTreeMap::new();
                let mut bytes = 0usize;
                let mut error = None;
                while let Ok(event) = rx.recv() {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("预览已取消".into());
                    }
                    match event {
                        conversation::Event::Text(v) => text.push_str(&v),
                        conversation::Event::StreamEnd(Err(e)) => error = Some(e),
                        conversation::Event::Audio(index, Ok((b, _))) => {
                            bytes += b.len();
                            if bytes > 64 * 1024 * 1024 {
                                return Err("预览音声缓存超限".into());
                            }
                            fs::create_dir_all(&audio_root).map_err(|e| e.to_string())?;
                            let path = audio_root.join(format!("{index}.mp3"));
                            fs::write(&path, b).map_err(|e| e.to_string())?;
                            audio.insert(index, json!({"path":path.to_string_lossy()}));
                        }
                        conversation::Event::Finished => break,
                        _ => {}
                    }
                }
                if text.trim().is_empty() {
                    if response["mode"] == "generated-with-fallback" {
                        let v = choice(&json!({"variants":response["fallback"]}))?;
                        return ayase_core::presentation::resolve(
                            &package,
                            &json!({}),
                            false,
                            &root,
                            &v,
                            "studio.preview",
                        );
                    }
                    return Err(error.unwrap_or_else(|| "没有生成正文".into()));
                }
                let (mut plan, mut channels) = ayase_core::presentation::resolve(
                    &package,
                    &json!({}),
                    false,
                    &root,
                    &json!({"text":text,"cue":response["cue"],"animationAsset":response["animationAsset"]}),
                    "studio.preview",
                )?;
                if !audio.is_empty() {
                    plan["audioSegments"] = json!(audio.into_values().collect::<Vec<_>>());
                    plan["audioDone"] = json!(true);
                    channels.push("audio".into());
                }
                Ok((plan, channels))
            })();
            let _ = tx.send(Prepared {
                project: owner,
                revision: epoch,
                id: task,
                generation,
                result,
            });
        });
    }
    s.tick();
    Ok(json!({"taskId":task,"realServices":real}))
}

#[cfg(test)]
mod tests {
    use super::super::tests::TestRoot;
    use super::*;
    #[test]
    fn imported_plugin_restart_uses_selected_hash_not_directory_order() {
        let root = TestRoot::new();
        let library = root.0.join("plugin-library");
        fs::create_dir_all(&library).unwrap();
        let mut project = Project::rule_plugin();
        let first = project.export().unwrap();
        let first_package = package::validate(&first).unwrap();
        project.document.manifest["version"] = json!("9.0.0");
        let newer = project.export().unwrap();
        let newer_package = package::validate(&newer).unwrap();
        fs::write(
            library.join(format!("{}.ayplug", first_package.report.hash)),
            first,
        )
        .unwrap();
        fs::write(
            library.join(format!("{}.ayplug", newer_package.report.hash)),
            newer,
        )
        .unwrap();
        fs::write(
            library.join("index.json"),
            serde_json::to_vec(&json!({first_package.report.id:first_package.report.hash}))
                .unwrap(),
        )
        .unwrap();
        let studio = new(root.0.clone()).unwrap();
        assert_eq!(
            studio.lock().unwrap().plugins["example.nightly-rest"].manifest["version"],
            "0.2.0"
        );
    }
    #[test]
    fn workspace_is_durable_hash_checked_and_does_not_export_author_metadata() {
        let root = TestRoot::new();
        let s = new(root.0.join("authoring")).unwrap();
        let mut s = s.lock().unwrap();
        let mut p = Project::theme();
        p.asset(
            "panel",
            "image",
            "assets/panel.png",
            include_bytes!("../../icons/icon.png").to_vec(),
        )
        .unwrap();
        let id = s.insert(p).unwrap();
        let w = s.working(&id).unwrap();
        let loaded = open_workspace(&w.root.join("project.json")).unwrap();
        assert_eq!(loaded.document.revision, 1);
        assert_eq!(loaded.files.len(), 1);
        assert!(!loaded
            .validated()
            .unwrap()
            .files
            .contains_key("project.json"));
        let mut bad: Value =
            serde_json::from_slice(&fs::read(w.root.join("project.json")).unwrap()).unwrap();
        bad["files"] = json!({"../escape":"bad"});
        fs::write(
            w.root.join("project.json"),
            serde_json::to_vec(&bad).unwrap(),
        )
        .unwrap();
        assert!(open_workspace(&w.root.join("project.json")).is_err());
    }
    #[test]
    fn interrupted_file_replacement_can_recover_prior_saved_bytes() {
        let root = TestRoot::new();
        let mut p = Project::theme();
        p.asset(
            "panel",
            "image",
            "panel.png",
            include_bytes!("../../icons/icon.png").to_vec(),
        )
        .unwrap();
        persist(&root.0, &p).unwrap();
        fs::write(root.0.join("files/panel.png"), b"unfinished replacement").unwrap();
        let loaded = open_workspace(&root.0.join("project.json")).unwrap();
        assert_eq!(loaded.files, p.files);
    }
    #[test]
    fn project_switch_revokes_preview_and_rejects_stale_edits() {
        let root = TestRoot::new();
        let s = new(root.0.clone()).unwrap();
        let mut s = s.lock().unwrap();
        let first = s.insert(Project::theme()).unwrap();
        let mut p = s.working(&first).unwrap().project.clone();
        let d = p.document.clone();
        p.save_document(0, d.clone()).unwrap();
        assert!(p.save_document(0, d).is_err());
        s.commit_project(&first, p).unwrap();
        let second = s.insert(Project::theme()).unwrap();
        assert_ne!(first, second);
        assert!(s.working(&first).is_err());
        assert!(s.preview.active().is_none());
    }
}
