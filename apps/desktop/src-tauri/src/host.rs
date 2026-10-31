use crate::environment::{self, Facts};
use crate::{conversation, memory, services};
use ayase_core::{
    config,
    package::{self, Package, PackageReport},
    plugin::{self, Configured, Output, Plugin, Runtime},
    scheduler::{Priority, Request, Scheduler},
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{Manager, State, WebviewWindow};
use uuid::Uuid;
mod hit_testing;
mod p2;
mod p3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Instance {
    id: String,
    package_hash: String,
    name: String,
    overrides: Value,
    revisions: BTreeMap<String, u64>,
    previous_packages: Vec<String>,
    draft: String,
    runtime: Runtime,
    #[serde(default)]
    profile: Option<conversation::Profile>,
    #[serde(default)]
    profile_revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BubblePreferences {
    animation: String,
    characters_per_second: u32,
    font_size: u32,
    read_ms: u32,
}
impl Default for BubblePreferences {
    fn default() -> Self {
        Self {
            animation: "typewriter".into(),
            characters_per_second: 30,
            font_size: 15,
            read_ms: 2800,
        }
    }
}
impl BubblePreferences {
    fn validate(&self) -> Result<(), String> {
        if !["typewriter", "none"].contains(&self.animation.as_str())
            || !(10..=80).contains(&self.characters_per_second)
            || !(12..=24).contains(&self.font_size)
            || !(1000..=10000).contains(&self.read_ms)
        {
            return Err("气泡设置无效：字号 12—24，速度 10—80 字/秒，停留 1000—10000 毫秒".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    active: Option<String>,
    instances: Vec<Instance>,
    library: Vec<PackageReport>,
    installed: Vec<String>,
    muted: bool,
    zoom: f64,
    excluded: Vec<String>,
    #[serde(default)]
    bubble: BubblePreferences,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            active: None,
            instances: vec![],
            library: vec![],
            installed: vec![],
            muted: false,
            zoom: 1.0,
            excluded: vec![],
            bubble: BubblePreferences::default(),
        }
    }
}
struct Host {
    root: PathBuf,
    db: Connection,
    store: Store,
    package: Option<Package>,
    plugins: BTreeMap<String, Plugin>,
    scheduler: Scheduler,
    origin: Instant,
    facts: Arc<Mutex<Facts>>,
    env_settings: Arc<Mutex<environment::Settings>>,
    previous_facts: (Option<bool>, Option<bool>),
    previous_media_sources: Vec<String>,
    last_wall: i64,
    startup_sent: bool,
    renderer_ready: bool,
    ai: p2::Ai,
    p3: p3::P3,
    app: Option<tauri::AppHandle>,
}
type Shared = Arc<Mutex<Host>>;
fn materialize_identity_assets(manifest: &mut Value, overrides: &Value) {
    if let Some(identity) = overrides.get("identity") {
        manifest["identity"] = config::effective(&manifest["identity"], &json!({}), identity);
        if identity.get("persona").is_some() {
            if let Some(identity) = manifest["identity"].as_object_mut() {
                identity.remove("personaAsset");
            }
        }
    }
    if let Some(assets) = overrides["assetEnabled"].as_object() {
        for (id, enabled) in assets {
            if manifest["assets"][id]["kind"] == "audio" && enabled.is_boolean() {
                manifest["assets"][id]["enabled"] = enabled.clone();
            }
        }
    }
}
impl Host {
    fn asset_enabled(&self, id: &str) -> bool {
        self.instance()
            .and_then(|i| i.overrides["assetEnabled"][id].as_bool())
            .or_else(|| {
                self.package
                    .as_ref()
                    .and_then(|p| p.manifest["assets"][id]["enabled"].as_bool())
            })
            .unwrap_or(true)
    }
    fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(root.join("packages")).map_err(|e| e.to_string())?;
        let db = Connection::open(root.join("host.sqlite3")).map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);").map_err(|e|e.to_string())?;
        let stored = db.query_row("SELECT value FROM settings WHERE key='host'", [], |r| {
            r.get::<_, String>(0)
        });
        if stored.is_ok() && !root.join("backups/before-p3.complete").exists() {
            let backup = root.join("backups/before-p3");
            fs::create_dir_all(&backup).map_err(|_| "P3升级前备份目录无法创建")?;
            let mut sources = vec![root.join("host.sqlite3"), root.join("services.sqlite3")];
            if let Ok(items) = fs::read_dir(root.join("contexts")) {
                for item in items.flatten() {
                    let path = item.path();
                    if path.extension().is_some_and(|x| x == "sqlite3") {
                        sources.push(path);
                    }
                }
            }
            for source in sources.into_iter().filter(|p| p.exists()) {
                let parent = if source.parent() == Some(root.join("contexts").as_path()) {
                    backup.join("contexts")
                } else {
                    backup.clone()
                };
                fs::create_dir_all(&parent).map_err(|_| "备份目录不可写")?;
                let destination = parent.join(source.file_name().unwrap());
                if !destination.exists() {
                    let source_db = Connection::open(&source).map_err(|_| "升级前数据库不可读")?;
                    source_db
                        .execute("VACUUM INTO ?1", [destination.to_string_lossy().as_ref()])
                        .map_err(|_| "P3升级前一致性备份失败，原数据保留")?;
                }
                let check = Connection::open(&destination).map_err(|_| "升级备份不可读")?;
                let result: String = check
                    .query_row("PRAGMA quick_check", [], |r| r.get(0))
                    .map_err(|_| "升级备份校验失败")?;
                if result != "ok" {
                    return Err("P3升级备份不完整，原数据保留".into());
                }
            }
            fs::write(
                root.join("backups/before-p3.complete"),
                "v0.2.0 databases preserved",
            )
            .map_err(|_| "升级备份标记不可写")?;
        }
        // Snapshot the existing P0/P1 settings before adding P2 metadata. Packages
        // are immutable and remain in place; this uses SQLite's consistent copy.
        if stored.is_ok() {
            let migrated: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM settings WHERE key='p2-backup-created')",
                    [],
                    |r| r.get(0),
                )
                .map_err(|_| "无法检查升级备份")?;
            if !migrated {
                let backup_dir = root.join("backups");
                fs::create_dir_all(&backup_dir).map_err(|_| "无法创建升级前档案备份")?;
                let backup = backup_dir.join("before-p2-host.sqlite3");
                if !backup.exists() {
                    db.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])
                        .map_err(|_| "无法备份升级前档案，原数据库已保留")?;
                }
                let check = Connection::open(&backup).map_err(|_| "升级备份不可读")?;
                let status: String = check
                    .query_row("PRAGMA quick_check", [], |r| r.get(0))
                    .map_err(|_| "升级备份校验失败")?;
                if status != "ok" {
                    return Err("升级备份不完整，原数据库已保留".into());
                }
                db.execute(
                    "INSERT INTO settings VALUES('p2-backup-created','true')",
                    [],
                )
                .map_err(|_| "升级标记保存失败")?;
            }
        }
        let store: Store = match stored {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("本地档案不可读，已保留原数据：{e}"))?,
            Err(rusqlite::Error::QueryReturnedNoRows) => Store::default(),
            Err(e) => return Err(e.to_string()),
        };
        let origin = Instant::now();
        let facts = Arc::new(Mutex::new(Facts::default()));
        let env_settings = Arc::new(Mutex::new(environment::Settings::default()));
        env_settings.lock().unwrap().excluded = store.excluded.clone();
        let ai = p2::Ai::open(&root)?;
        let mut h = Self {
            root,
            db,
            store,
            package: None,
            plugins: plugin::builtin(),
            scheduler: Scheduler::new(""),
            origin,
            facts,
            env_settings,
            previous_facts: (None, None),
            previous_media_sources: vec![],
            last_wall: 0,
            startup_sent: false,
            renderer_ready: false,
            ai,
            p3: p3::P3::default(),
            app: None,
        };
        for hash in &h.store.installed {
            if let Ok(p) = h.load_package(hash) {
                if p.report.kind == "plugin" {
                    if let Ok(plugin) = Plugin::from_files(p.manifest.clone(), &p.files) {
                        h.plugins.insert(p.report.id, plugin);
                    }
                }
            }
        }
        if let Some(i) = h.instance() {
            let id = i.id.clone();
            let hash = i.package_hash.clone();
            match h.load_package(&hash) {
                Ok(p) => {
                    h.package = Some(p);
                    h.scheduler = Scheduler::new(&id)
                }
                Err(e) => h.scheduler.record(0, "restore-failed", json!(e)),
            }
        }
        h.scheduler.muted = h.store.muted;
        for i in &mut h.store.instances {
            for (key, value) in &mut i.runtime.state {
                if key.starts_with("p3:") && key != "p3:budget" {
                    if let Some(o) = value.as_object_mut() {
                        o.remove("accepted");
                        o.insert("work".into(), json!(0));
                        o.insert("scheduled".into(), json!(false));
                        o.insert("last".into(), json!(0));
                    }
                }
            }
        }
        for i in &h.store.instances {
            if h.ai.memory.finalize_interrupted(&i.id).is_err() {
                h.ai.notice = "部分伙伴上下文暂不可读，原数据已保留".into();
            }
        }
        Ok(h)
    }
    fn now(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }
    fn instance(&self) -> Option<&Instance> {
        self.store
            .instances
            .iter()
            .find(|i| Some(&i.id) == self.store.active.as_ref())
    }
    fn instance_mut(&mut self) -> Option<&mut Instance> {
        self.store
            .instances
            .iter_mut()
            .find(|i| Some(&i.id) == self.store.active.as_ref())
    }
    fn commit(&mut self, new: Store) -> Result<(), String> {
        let text = serde_json::to_string(&new).map_err(|e| e.to_string())?;
        self.db.execute("INSERT INTO settings(key,value) VALUES('host',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![text]).map_err(|e|e.to_string())?;
        self.store = new;
        Ok(())
    }
    fn stage_archive(&mut self, bytes: &[u8]) -> Result<PackageReport, String> {
        let p = package::validate(bytes)?;
        let hash = &p.report.hash;
        let final_dir = self.root.join("packages").join(hash);
        if !final_dir.exists() {
            let staging = self
                .root
                .join("packages")
                .join(format!(".staging-{}", Uuid::new_v4()));
            let result = (|| {
                p.write_to(&staging.join("content"))?;
                fs::write(staging.join("archive.zip"), bytes).map_err(|e| e.to_string())?;
                fs::rename(&staging, &final_dir).map_err(|e| e.to_string())
            })();
            if result.is_err() {
                let _ = fs::remove_dir_all(&staging);
            }
            result?;
        }
        let mut store = self.store.clone();
        if !store.library.iter().any(|r| r.hash == *hash) {
            store.library.push(p.report.clone());
        }
        // Staging never activates a character or installs a plugin.
        self.commit(store)?;
        Ok(p.report)
    }
    fn save_draft(&mut self, instance_id: &str, text: &str) -> Result<(), String> {
        if text.len() > 48_000 {
            return Err("草稿过长".into());
        }
        let mut store = self.store.clone();
        let instance = store
            .instances
            .iter_mut()
            .find(|i| i.id == instance_id)
            .ok_or("草稿所属伙伴不存在")?;
        instance.draft = text.into();
        self.commit(store)
    }
    fn rename_instance(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
            return Err("伙伴名称需为 1—40 个字符，不能包含换行或控制字符".into());
        }
        let mut store = self.store.clone();
        store
            .instances
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or("伙伴不存在")?
            .name = name.into();
        self.commit(store)
    }
    fn load_package(&self, hash: &str) -> Result<Package, String> {
        if !hash.bytes().all(|b| b.is_ascii_hexdigit()) || hash.len() != 64 {
            return Err("无效包标识".into());
        }
        let b = fs::read(self.root.join("packages").join(hash).join("archive.zip"))
            .map_err(|e| e.to_string())?;
        if package::digest(&b) != hash {
            return Err("本地包散列不一致".into());
        }
        package::validate(&b)
    }
    fn presets(&self) -> Vec<Value> {
        let Some(p) = &self.package else {
            return vec![];
        };
        let base = p.manifest["pluginPresets"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let extra = self
            .instance()
            .and_then(|i| i.overrides["extraPresets"].as_array())
            .cloned()
            .unwrap_or_default();
        base.into_iter().chain(extra).collect()
    }
    fn configured(&self) -> Vec<Configured> {
        let Some(i) = self.instance() else {
            return vec![];
        };
        let empty = json!({});
        self.presets()
            .iter()
            .filter_map(|preset| {
                let id = preset["pluginId"].as_str()?;
                let owner = preset["presetId"].as_str()?;
                self.plugins.get(id).map(|p| {
                    p.configure(
                        preset,
                        i.overrides["plugins"].get(owner).unwrap_or(&empty),
                        *i.revisions.get(owner).unwrap_or(&0),
                    )
                })
            })
            .collect()
    }
    fn dispatch(&mut self, event: &str, active: Option<bool>, wall: DateTime<Utc>) {
        let configs = self.configured();
        let mut outputs = vec![];
        let mut runtime = self
            .instance()
            .map(|i| i.runtime.clone())
            .unwrap_or_default();
        for c in configs {
            if let Some(p) = self.plugins.get(&c.plugin_id) {
                let filtered_active = if event == "environment.media"
                    && active == Some(true)
                    && c.config["sources"]
                        .as_array()
                        .is_some_and(|v| !v.is_empty())
                {
                    let f = self.facts.lock().unwrap();
                    Some(
                        c.config["sources"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter_map(Value::as_str)
                            .any(|filter| {
                                f.media_sources.iter().any(|source| {
                                    source.to_lowercase().contains(&filter.to_lowercase())
                                })
                            }),
                    )
                } else {
                    active
                };
                for output in p.dispatch(&c, event, filtered_active, wall, &mut runtime) {
                    outputs.push((c.clone(), output));
                }
            }
        }
        if let Some(i) = self.instance_mut() {
            i.runtime = runtime;
        }
        for (c, output) in outputs {
            match output {
                Output::Present(response, ttl) => {
                    let random = (Uuid::new_v4().as_u128() >> 64) as f64 / u64::MAX as f64;
                    if response["mode"] != "fixed" {
                        if let Err(e) = self
                            .generate(response, &c.owner, c.priority, true, ttl, c.revision, None)
                        {
                            self.ai.notice = e;
                        }
                    } else if let Some(v) = plugin::choose_variant(&response, random) {
                        if let Err(e) = self.present(v, &c.owner, c.priority, true, ttl, c.revision)
                        {
                            let now = self.now();
                            self.scheduler.record(
                                now,
                                "request-skipped",
                                json!({"owner":c.owner,"reason":e}),
                            );
                        }
                    }
                }
                Output::Claim(mode, suppress) => {
                    self.scheduler.claim(&c.owner, &mode, c.priority, suppress)
                }
                Output::Release => {
                    self.scheduler.claims.remove(&c.owner);
                }
            }
        }
    }
    fn resolve_variant(&self, v: &Value, owner: &str) -> Result<(Value, Vec<String>), String> {
        let p = self.package.as_ref().ok_or("请先激活角色包")?;
        let i = self.instance().ok_or("没有活动角色")?;
        let mut plan = json!({"repeat":1});
        if let Some(read_ms) = v["readMs"].as_u64() {
            plan["readMs"] = json!(read_ms);
        }
        let mut channels = vec![];
        let text = v["text"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| v["textAsset"].as_str().and_then(|id| p.asset_text(id)));
        if let Some(text) = text.filter(|s| !s.trim().is_empty()) {
            plan["text"] = json!(text);
            channels.push("text".into());
        }
        if let Some(id) = v["audioAsset"].as_str() {
            if let Some(path) = p.manifest["assets"][id]["path"]
                .as_str()
                .filter(|_| self.asset_enabled(id))
            {
                if p.manifest["assets"][id]["kind"] == "audio"
                    && p.files.contains_key(path)
                    && !self.store.muted
                {
                    plan["audioPath"] = json!(self
                        .root
                        .join("packages")
                        .join(&p.report.hash)
                        .join("content")
                        .join(path)
                        .to_string_lossy());
                    plan["audioDurationMs"] = p.manifest["assets"][id]["durationMs"].clone();
                    channels.push("audio".into());
                }
            }
        }
        if let Some(cue) = v["cue"].as_str() {
            let model = p.model.as_ref().unwrap();
            let mapping = i.overrides["semanticCues"]
                .get(cue)
                .or_else(|| model["semanticCues"].get(cue));
            if let Some(mapping) = mapping {
                if let Some(clip) = mapping["clip"].as_str() {
                    if model["clips"][clip].is_object() {
                        plan["clips"] = json!([clip]);
                        plan["repeat"] = json!(mapping["repeat"].as_u64().unwrap_or(1).clamp(1, 8));
                        channels.push("animation".into());
                    }
                }
            }
            // Explicit UI clip preview is represented as a host-only cue.
            if owner == "host.user" && cue.starts_with("clip:") {
                let clip = &cue[5..];
                if model["clips"][clip].is_object() {
                    plan["clips"] = json!([clip]);
                    channels.push("animation".into());
                }
            }
        }
        Ok((plan, channels))
    }
    fn present(
        &mut self,
        v: Value,
        owner: &str,
        priority: Priority,
        proactive: bool,
        ttl: u64,
        revision: u64,
    ) -> Result<u64, String> {
        let (plan, channels) = self.resolve_variant(&v, owner)?;
        let i = self.instance().ok_or("没有活动角色")?;
        self.scheduler.submit(
            Request {
                instance_id: i.id.clone(),
                owner: owner.into(),
                revision,
                priority,
                proactive,
                ttl_ms: ttl,
                preparing: false,
                plan,
                channels,
                dedupe: None,
            },
            self.now(),
        )
    }
    fn tick(&mut self) {
        let now = self.now();
        self.ai_tick();
        let wall = Utc::now();
        if self.package.is_none() {
            return;
        }
        for c in self.configured() {
            self.scheduler.revisions.insert(c.owner, c.revision);
        }
        if self.last_wall != wall.timestamp() {
            for event in environment::drain_events() {
                self.dispatch(&event, None, wall);
            }
            let mut facts = self.facts.lock().unwrap().clone();
            if now.saturating_sub(facts.sample) > 5000 {
                facts.fullscreen = None;
                facts.media = None;
            }
            if self.previous_facts.0 != facts.fullscreen {
                self.dispatch("environment.fullscreen", facts.fullscreen, wall);
                self.scheduler
                    .record(now, "fullscreen-fact", json!(facts.fullscreen));
            }
            if self.previous_facts.1 != facts.media
                || self.previous_media_sources != facts.media_sources
            {
                self.dispatch("environment.media", facts.media, wall);
                self.scheduler.record(now, "media-fact", json!(facts.media));
            }
            self.previous_facts = (facts.fullscreen, facts.media);
            self.previous_media_sources = facts.media_sources;
            let before = self.instance().map(|i| i.runtime.occurrences.clone());
            self.dispatch("clock.tick", None, wall);
            if let Some(i) = self.instance_mut() {
                i.runtime.last_wall = Some(wall.timestamp());
            }
            if before != self.instance().map(|i| i.runtime.occurrences.clone()) {
                if let Err(e) = self.commit(self.store.clone()) {
                    self.scheduler.record(now, "save-failed", json!(e));
                }
            }
            if self.last_wall != 0 && wall.timestamp() - self.last_wall > 5 {
                self.scheduler.record(now,"system-time-gap",json!({"seconds":wall.timestamp()-self.last_wall,"note":"恢复后的真实通道行为需实机记录"}));
            }
            self.last_wall = wall.timestamp();
            let app = self.app.clone();
            self.p3_tick(wall, None, app.as_ref());
        }
        if let Some(model) = self.package.as_ref().and_then(|p| p.model.as_ref()) {
            self.scheduler.reconcile_base(model, now);
        }
        if self.renderer_ready {
            self.scheduler.tick(now);
        }
    }
    fn snapshot(&self) -> Value {
        let asset_paths = self
            .package
            .as_ref()
            .map(|p| {
                p.manifest["assets"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .filter_map(|(id, v)| {
                        let path = v["path"].as_str()?;
                        if !p.files.contains_key(path) {
                            return None;
                        }
                        Some((
                            id.clone(),
                            json!(self
                                .root
                                .join("packages")
                                .join(&p.report.hash)
                                .join("content")
                                .join(path)
                                .to_string_lossy()),
                        ))
                    })
                    .collect::<serde_json::Map<_, _>>()
            })
            .unwrap_or_default();
        let plugins=self.plugins.values().map(|p|json!({"id":p.manifest["pluginId"],"name":p.manifest["displayName"],"defaults":p.manifest["defaults"],"hostBehavior":p.manifest["hostBehavior"],"schema":p.schema,"configVersion":p.manifest["configVersion"],"priority":p.manifest["defaultPriority"]})).collect::<Vec<_>>();
        let known = self.configured();
        let unknown = self
            .presets()
            .into_iter()
            .filter(|p| {
                !self
                    .plugins
                    .contains_key(p["pluginId"].as_str().unwrap_or(""))
            })
            .collect::<Vec<_>>();
        let mut facts = self.facts.lock().unwrap().clone();
        if self.now().saturating_sub(facts.sample) > 5000 {
            facts.fullscreen = None;
            facts.media = None;
            facts.fullscreen_status = "采集超时 / 状态未知".into();
            facts.media_status = "采集超时 / 状态未知".into();
        }
        json!({"version":"0.3.0","stage":"P3 · 实机验收候选","p3":self.p3_snapshot(),"ai":self.ai_snapshot(),"instance":self.instance(),"instances":self.store.instances.iter().map(|i|json!({"id":i.id,"name":i.name,"packageHash":i.package_hash})).collect::<Vec<_>>(),"library":self.store.library,
            "package":self.package.as_ref().map(|p|&p.report),"manifest":self.package.as_ref().map(|p|&p.manifest),"model":self.package.as_ref().and_then(|p|p.model.as_ref()),"assetPaths":asset_paths,
            "plugins":plugins,"configured":known,"unknownPresets":unknown,"scheduler":self.scheduler.snapshot(),"facts":facts,"zoom":self.store.zoom,"bubble":self.store.bubble,"excluded":self.store.excluded,"dataDirectory":self.root.to_string_lossy()})
    }
}
fn control(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "control" {
        Ok(())
    } else {
        Err("此操作仅允许控制台调用".into())
    }
}

#[tauri::command]
fn snapshot(state: State<Shared>, regions: State<Arc<hit_testing::Regions>>) -> Value {
    let mut value = state.lock().unwrap().snapshot();
    value["petInteractive"] = json!(regions.0.lock().unwrap().hovered);
    value
}
#[tauri::command]
async fn import_package(
    window: WebviewWindow,
    state: State<'_, Shared>,
    path: String,
) -> Result<Value, String> {
    control(&window)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
        if metadata.len() > package::MAX_TOTAL {
            return Err("压缩包超过限制".into());
        }
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let report = state.lock().unwrap().stage_archive(&bytes)?;
        Ok(json!(report))
    })
    .await
    .map_err(|e| e.to_string())?
}
fn activate(h: &mut Host, hash: &str, mode: &str) -> Result<(), String> {
    if mode == "update"
        && h.instance().is_some_and(|i| i.package_hash == hash)
        && h.package.is_some()
    {
        return Ok(());
    }
    let p = h.load_package(hash)?;
    if p.report.kind != "character" {
        return Err("请选择角色包".into());
    }
    let mut store = h.store.clone();
    let id;
    if mode == "update" {
        let current = h.instance().ok_or("没有可更新的伙伴")?;
        let existing = h.package.as_ref().ok_or("当前包未加载")?;
        if existing.report.id != p.report.id {
            return Err("更新需使用相同 packageId；换用其他包请创建新伙伴".into());
        }
        id = current.id.clone();
        let item = store.instances.iter_mut().find(|i| i.id == id).unwrap();
        if item.package_hash != hash {
            item.previous_packages.push(item.package_hash.clone());
            item.package_hash = hash.into();
        }
    } else if mode == "new" {
        id = Uuid::new_v4().to_string();
        store.instances.push(Instance {
            id: id.clone(),
            package_hash: hash.into(),
            name: p.report.display_name.clone(),
            overrides: json!({"plugins":{},"extraPresets":[]}),
            revisions: BTreeMap::new(),
            previous_packages: vec![],
            draft: String::new(),
            runtime: Runtime::default(),
            profile: None,
            profile_revision: 0,
        });
    } else {
        return Err("请选择 new 或 update".into());
    }
    store.active = Some(id.clone());
    h.commit(store)?;
    let now = h.now();
    h.scheduler.switch_instance(&id, now);
    h.package = Some(p);
    h.previous_facts = (None, None);
    // Activation is deliberately distinct from app.ready. Only the first ready
    // renderer of this process emits app.ready, including first-run activation.
    h.last_wall = 0;
    h.renderer_ready = false;
    Ok(())
}
#[tauri::command]
fn activate_package(
    window: WebviewWindow,
    state: State<Shared>,
    hash: String,
    mode: String,
) -> Result<(), String> {
    control(&window)?;
    let mut h = state.lock().unwrap();
    activate(&mut h, &hash, &mode)?;
    allow_assets(&window.app_handle(), &h);
    Ok(())
}
#[tauri::command]
fn select_instance(window: WebviewWindow, state: State<Shared>, id: String) -> Result<(), String> {
    control(&window)?;
    let mut h = state.lock().unwrap();
    if h.store.active.as_deref() == Some(&id) && h.package.is_some() {
        return Ok(());
    }
    let hash = h
        .store
        .instances
        .iter()
        .find(|i| i.id == id)
        .ok_or("伙伴不存在")?
        .package_hash
        .clone();
    let p = h.load_package(&hash)?;
    let mut store = h.store.clone();
    store.active = Some(id.clone());
    h.commit(store)?;
    let now = h.now();
    h.scheduler.switch_instance(&id, now);
    h.package = Some(p);
    h.previous_facts = (None, None);
    h.last_wall = 0;
    h.renderer_ready = false;
    allow_assets(window.app_handle(), &h);
    Ok(())
}
#[tauri::command]
fn rollback_package(window: WebviewWindow, state: State<Shared>) -> Result<(), String> {
    control(&window)?;
    let mut h = state.lock().unwrap();
    let old = h
        .instance()
        .and_then(|i| i.previous_packages.last())
        .ok_or("没有旧包可回退")?
        .clone();
    activate(&mut h, &old, "update")?;
    allow_assets(window.app_handle(), &h);
    Ok(())
}
fn allow_assets(app: &tauri::AppHandle, h: &Host) {
    let _ = app
        .asset_protocol_scope()
        .allow_directory(h.root.join("audio-cache"), true);
    if let Some(p) = &h.package {
        let _ = app.asset_protocol_scope().allow_directory(
            h.root.join("packages").join(&p.report.hash).join("content"),
            true,
        );
    }
}
#[tauri::command]
fn install_plugin(window: WebviewWindow, state: State<Shared>, hash: String) -> Result<(), String> {
    control(&window)?;
    let mut h = state.lock().unwrap();
    let p = h.load_package(&hash)?;
    if p.report.kind != "plugin" {
        return Err("请选择插件包".into());
    }
    if plugin::builtin().contains_key(&p.report.id) {
        return Err("内置插件不接受外部包覆盖".into());
    }
    let plugin = Plugin::from_files(p.manifest.clone(), &p.files)?;
    let mut store = h.store.clone();
    let old_hashes = store
        .library
        .iter()
        .filter(|r| r.kind == "plugin" && r.id == p.report.id)
        .map(|r| r.hash.clone())
        .collect::<Vec<_>>();
    store.installed.retain(|x| !old_hashes.contains(x));
    store.installed.push(hash);
    h.commit(store)?;
    h.plugins.insert(p.report.id, plugin);
    h.last_wall = 0;
    h.previous_facts = (None, None);
    Ok(())
}
#[tauri::command]
fn uninstall_plugin(
    window: WebviewWindow,
    state: State<Shared>,
    plugin_id: String,
) -> Result<(), String> {
    control(&window)?;
    let mut h = state.lock().unwrap();
    if plugin::builtin().contains_key(&plugin_id) {
        return Err("内置插件请通过角色配置关闭".into());
    }
    let owners = h
        .configured()
        .into_iter()
        .filter(|c| c.plugin_id == plugin_id)
        .map(|c| c.owner)
        .collect::<Vec<_>>();
    let hashes = h
        .store
        .library
        .iter()
        .filter(|r| r.id == plugin_id && r.kind == "plugin")
        .map(|r| r.hash.clone())
        .collect::<Vec<_>>();
    let mut store = h.store.clone();
    store.installed.retain(|x| !hashes.contains(x));
    h.commit(store)?;
    h.plugins.remove(&plugin_id);
    let now = h.now();
    for owner in owners {
        h.scheduler.revoke_owner(&owner, now);
    }
    Ok(())
}
#[tauri::command]
fn save_preset(window: WebviewWindow, state: State<Shared>, preset: Value) -> Result<(), String> {
    control(&window)?;
    let mut h = state.lock().unwrap();
    let id = preset["presetId"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or("缺少配置 ID")?
        .to_owned();
    let plugin_id = preset["pluginId"].as_str().ok_or("缺少插件 ID")?;
    let p = h
        .plugins
        .get(plugin_id)
        .ok_or("插件尚未安装；未知配置将原样保留")?;
    p.validate_config(&preset["config"])?;
    if preset["configVersion"] != p.manifest["configVersion"] {
        return Err("配置版本不兼容".into());
    }
    Priority::plugin(preset["priority"].as_str().ok_or("缺少优先级")?)?;
    let enabled = preset["enabled"].as_bool().ok_or("enabled 需要布尔值")?;
    let existing = h.presets().into_iter().find(|p| p["presetId"] == id);
    if existing
        .as_ref()
        .is_some_and(|p| p["pluginId"] != plugin_id)
    {
        return Err("现有配置不能改绑插件 ID".into());
    }
    let base_defaults = if existing
        .as_ref()
        .is_some_and(|p| p["configMode"] == "complete")
    {
        json!({})
    } else {
        p.manifest["defaults"].clone()
    };
    let base_config = config::effective(
        &base_defaults,
        &existing
            .as_ref()
            .map(|p| p["config"].clone())
            .unwrap_or(json!({})),
        &json!({}),
    );
    let saved_config = config::from_form(&base_config, &preset["config"]);
    let mut store = h.store.clone();
    let i = store
        .instances
        .iter_mut()
        .find(|i| Some(&i.id) == store.active.as_ref())
        .ok_or("请先激活角色")?;
    if existing.is_none() {
        i.overrides["extraPresets"]
            .as_array_mut()
            .unwrap()
            .push(preset.clone());
    }
    i.overrides["plugins"][&id] =
        json!({"enabled":enabled,"priority":preset["priority"],"config":saved_config});
    *i.revisions.entry(id.clone()).or_insert(0) += 1;
    h.commit(store)?;
    let now = h.now();
    if !enabled {
        h.scheduler.revoke_owner(&id, now);
    } else {
        // Config edits change pending work; already started presentation keeps ownership.
        h.scheduler.claims.remove(&id);
        h.previous_facts = (None, None);
        h.last_wall = 0;
    }
    Ok(())
}
#[tauri::command]
async fn export_package(
    window: WebviewWindow,
    state: State<'_, Shared>,
    path: String,
    version: String,
) -> Result<String, String> {
    control(&window)?;
    let (p, overrides, configs, known) = {
        let h = state.lock().unwrap();
        (
            h.package.clone().ok_or("请先激活角色")?,
            h.instance().unwrap().overrides.clone(),
            h.presets(),
            h.configured(),
        )
    };
    tauri::async_runtime::spawn_blocking(move || {
        let mut manifest = p.manifest.clone();
        manifest["version"] = json!(version);
        let mut presets = vec![];
        for mut preset in configs {
            let id = preset["presetId"].as_str().unwrap().to_string();
            if let Some(patch) = overrides["plugins"].get(&id) {
                preset = config::effective(&preset, &json!({}), patch)
            }
            if let Some(c) = known.iter().find(|c| c.owner == id) {
                if c.status == "已启用" || c.status == "已关闭" {
                    preset["config"] = c.config.clone();
                    preset["configMode"] = json!("complete");
                }
            }
            presets.push(preset);
        }
        manifest["pluginPresets"] = json!(presets);
        materialize_identity_assets(&mut manifest, &overrides);
        let bytes = package::pack(p.files.clone(), manifest, "manifest.json")?;
        package::validate(&bytes)?;
        let target = PathBuf::from(path);
        if target.exists() {
            return Err("为保留旧版本，请选择一个尚不存在的导出文件名".into());
        }
        let temp = target.with_extension(format!("tmp-{}", Uuid::new_v4()));
        fs::write(&temp, &bytes).map_err(|e| e.to_string())?;
        if let Err(e) = fs::rename(&temp, &target) {
            let _ = fs::remove_file(&temp);
            return Err(e.to_string());
        }
        Ok(package::digest(&bytes))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn manual_request(
    window: WebviewWindow,
    state: State<Shared>,
    variant: Value,
    priority: String,
    proactive: bool,
    instance_id: Option<String>,
) -> Result<u64, String> {
    if !["control", "input"].contains(&window.label()) {
        return Err("不允许的入口".into());
    }
    plugin::validate_response(&json!({"mode":"fixed","variants":[variant.clone()]}))?;
    let (owner, p) = if proactive {
        ("host.demo", Priority::plugin(&priority)?)
    } else {
        ("host.user", Priority::User)
    };
    let mut h = state.lock().unwrap();
    if window.label() == "input" && instance_id.as_deref() != h.store.active.as_deref() {
        return Err("伙伴已切换，请确认当前输入后重新发送".into());
    }
    h.present(variant, owner, p, proactive, 300_000, 0)
}
#[tauri::command]
fn presentation_ack(
    window: WebviewWindow,
    state: State<Shared>,
    id: u64,
    generation: u64,
    channel: String,
    event: String,
    visual_base: Option<String>,
    visual_kind: Option<String>,
) -> Result<bool, String> {
    if window.label() != "pet" {
        return Err("只有呈现窗口可以发送回执".into());
    }
    let mut h = state.lock().unwrap();
    let now = h.now();
    let accepted = match event.as_str() {
        "started" => h.generation_start_valid(id) && h.scheduler.started(id, generation, now),
        "completed" => h.scheduler.channel(id, generation, &channel, false, now),
        "failed" => h.scheduler.channel(id, generation, &channel, true, now),
        "valid" => h.scheduler.valid(id, generation),
        _ => false,
    };
    if accepted && channel == "animation" && event == "completed" {
        if let (Some(base), Some(kind)) = (visual_base, visual_kind) {
            if ["idle", "music", "quiet"].contains(&base.as_str())
                && ["base", "finite"].contains(&kind.as_str())
            {
                h.scheduler.record(
                    now,
                    "animation-return",
                    json!({"taskId":id,"base":base,"kind":kind}),
                );
            }
        }
    }
    Ok(accepted)
}
#[tauri::command]
fn base_transition_ack(
    window: WebviewWindow,
    state: State<Shared>,
    id: u64,
    generation: u64,
    event: String,
    visual_kind: Option<String>,
) -> Result<bool, String> {
    if window.label() != "pet" {
        return Err("只有呈现窗口可以发送回执".into());
    }
    let mut h = state.lock().unwrap();
    let now = h.now();
    let accepted = h.scheduler.base_transition_ack(id, generation, &event, now);
    if accepted && event == "completed" && visual_kind.as_deref() == Some("finite") {
        h.scheduler.record(
            now,
            "base-transition-rendered",
            json!({"id":id,"kind":"finite"}),
        );
    }
    Ok(accepted)
}
#[tauri::command]
fn renderer_ready(
    window: WebviewWindow,
    state: State<Shared>,
    instance_id: String,
    package_hash: String,
) -> Result<(), String> {
    if window.label() != "pet" {
        return Err("不允许的入口".into());
    }
    let mut h = state.lock().unwrap();
    if h.scheduler.current_instance != instance_id
        || h.instance().is_none_or(|i| i.package_hash != package_hash)
    {
        return Ok(());
    }
    h.renderer_ready = true;
    if !h.startup_sent {
        h.startup_sent = true;
        h.dispatch("app.ready", None, Utc::now());
    }
    Ok(())
}
#[tauri::command]
fn user_control(
    window: WebviewWindow,
    state: State<Shared>,
    action: String,
    value: Value,
) -> Result<(), String> {
    let mut h = state.lock().unwrap();
    let now = h.now();
    match action.as_str() {
        "rename" => {
            control(&window)?;
            h.rename_instance(
                value["instanceId"].as_str().ok_or("缺少伙伴身份")?,
                value["name"].as_str().ok_or("缺少名称")?,
            )?;
        }
        "bubble" => {
            control(&window)?;
            let preferences: BubblePreferences =
                serde_json::from_value(value).map_err(|e| e.to_string())?;
            preferences.validate()?;
            let mut store = h.store.clone();
            store.bubble = preferences;
            h.commit(store)?;
        }
        "stop" => {
            h.scheduler.stop(now);
            h.cancel_generations();
        }
        "mute" => {
            let muted = value.as_bool().ok_or("需要布尔值")?;
            let mut store = h.store.clone();
            store.muted = muted;
            h.commit(store)?;
            h.scheduler.mute(muted, now);
        }
        "zoom" => {
            control(&window)?;
            let z = value
                .as_f64()
                .filter(|z| (0.6..=1.6).contains(z))
                .ok_or("缩放应为 0.6—1.6")?;
            let mut store = h.store.clone();
            store.zoom = z;
            h.commit(store)?;
            resize_pet(window.app_handle(), z);
            clamp_pet(window.app_handle(), false);
        }
        "excluded" => {
            control(&window)?;
            let excluded: Vec<String> = serde_json::from_value(value).map_err(|e| e.to_string())?;
            if excluded.len() > 64 || excluded.iter().any(|x| x.len() > 160) {
                return Err("排除列表过长".into());
            }
            let mut store = h.store.clone();
            store.excluded = excluded.clone();
            h.commit(store)?;
            h.env_settings.lock().unwrap().excluded = excluded;
        }
        "draft" => {
            if window.label() != "input" {
                return Err("不允许的入口".into());
            }
            let instance_id = value["instanceId"].as_str().ok_or("草稿缺少伙伴身份")?;
            let text = value["text"].as_str().ok_or("草稿需要文字")?;
            h.save_draft(instance_id, text)?;
        }
        "simulate-time" => {
            control(&window)?;
            let text = value.as_str().ok_or("需要 ISO 8601 时刻")?;
            let wall = DateTime::parse_from_rfc3339(text)
                .map_err(|e| e.to_string())?
                .with_timezone(&Utc);
            let old = h.instance().map(|i| i.runtime.clone());
            if let Some(i) = h.instance_mut() {
                i.runtime = Runtime::default();
            }
            h.dispatch("clock.tick", None, wall);
            if let (Some(old), Some(i)) = (old, h.instance_mut()) {
                i.runtime = old;
            }
            h.scheduler.record(now, "simulated-time", json!(text));
        }
        "greeting" => {
            control(&window)?;
            h.dispatch("app.ready", None, Utc::now());
            h.scheduler.record(now, "manual-event", json!("app.ready"));
        }
        _ => return Err("未知用户操作".into()),
    }
    Ok(())
}
#[tauri::command]
fn show_window(window: WebviewWindow, label: String) -> Result<(), String> {
    if !["input", "control", "pet"].contains(&label.as_str()) {
        return Err("未知窗口".into());
    }
    let target = window
        .app_handle()
        .get_webview_window(&label)
        .ok_or("窗口不存在")?;
    if label == "input" {
        position_input(window.app_handle());
    }
    target.show().map_err(|e| e.to_string())?;
    if label != "pet" {
        target.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn clamp_pet(app: &tauri::AppHandle, reset: bool) {
    let Some(pet) = app.get_webview_window("pet") else {
        return;
    };
    let monitor = if reset {
        pet.primary_monitor()
    } else {
        pet.current_monitor()
    };
    if let Ok(Some(m)) = monitor {
        let work = m.work_area();
        let Ok(size) = pet.outer_size() else { return };
        let pos = pet.outer_position().unwrap_or(*m.position());
        let x = if reset {
            work.position.x + work.size.width as i32 - size.width as i32 - 32
        } else {
            pos.x
        };
        let y = if reset {
            work.position.y + work.size.height as i32 - size.height as i32 - 24
        } else {
            pos.y
        };
        let x = x.clamp(
            work.position.x,
            (work.position.x + work.size.width as i32 - size.width as i32).max(work.position.x),
        );
        let y = y.clamp(
            work.position.y,
            (work.position.y + work.size.height as i32 - size.height as i32).max(work.position.y),
        );
        if pos.x != x || pos.y != y {
            let _ = pet.set_position(tauri::PhysicalPosition::new(x, y));
        }
    }
}
fn resize_pet(app: &tauri::AppHandle, zoom: f64) {
    let Some(pet) = app.get_webview_window("pet") else {
        return;
    };
    let old = pet.outer_position().ok().zip(pet.outer_size().ok());
    let scale = pet.scale_factor().unwrap_or(1.0);
    let mut width = (360.0 * zoom + 40.0).max(420.0);
    let mut height = 68.0 + 560.0 * zoom;
    if let Ok(Some(monitor)) = pet.current_monitor() {
        width = width.min(monitor.work_area().size.width as f64 / scale);
        height = height.min(monitor.work_area().size.height as f64 / scale);
    }
    if pet.set_size(tauri::LogicalSize::new(width, height)).is_ok() {
        if let Some((position, size)) = old {
            let _ = pet.set_position(tauri::PhysicalPosition::new(
                position.x + (size.width as f64 / 2.0 - width * scale / 2.0).round() as i32,
                position.y + (size.height as f64 - height * scale).round() as i32,
            ));
        }
        clamp_pet(app, false);
    }
}
fn position_input(app: &tauri::AppHandle) {
    let (Some(pet), Some(input)) = (
        app.get_webview_window("pet"),
        app.get_webview_window("input"),
    ) else {
        return;
    };
    if let (Ok(pos), Ok(size), Ok(Some(m)), Ok(isize)) = (
        pet.outer_position(),
        pet.outer_size(),
        pet.current_monitor(),
        input.outer_size(),
    ) {
        let w = m.work_area();
        let x = pos.x.clamp(
            w.position.x,
            (w.position.x + w.size.width as i32 - isize.width as i32).max(w.position.x),
        );
        let y = (pos.y + size.height as i32)
            .min(w.position.y + w.size.height as i32 - isize.height as i32)
            .max(w.position.y);
        let _ = input.set_position(tauri::PhysicalPosition::new(x, y));
    }
}
#[tauri::command]
fn reset_position(window: WebviewWindow) {
    clamp_pet(window.app_handle(), true);
    position_input(window.app_handle());
}
#[tauri::command]
fn export_diagnostics(
    window: WebviewWindow,
    state: State<Shared>,
    path: String,
) -> Result<(), String> {
    control(&window)?;
    let h = state.lock().unwrap();
    let d = json!({"version":env!("CARGO_PKG_VERSION"),"package":h.package.as_ref().map(|p|&p.report),"log":h.scheduler.log,"facts":*h.facts.lock().unwrap(),"capturedAt":Utc::now().to_rfc3339()});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    use std::io::Write;
    file.write_all(serde_json::to_string_pretty(&d).unwrap().as_bytes())
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn exit_app(window: WebviewWindow) {
    window.app_handle().exit(0);
}

pub fn run() {
    let isolated_smoke = cfg!(feature = "smoke-test")
        && std::env::var_os("AYASE_SMOKE_PACKAGE").is_some()
        && std::env::var_os("AYASE_TEST_DATA_DIR").is_some();
    let mut builder = tauri::Builder::default();
    if !isolated_smoke {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("control") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }));
    }
    builder
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = if let Ok(custom) = std::env::var("AYASE_TEST_DATA_DIR") {
                PathBuf::from(custom)
            } else {
                app.path().app_data_dir()?
            };
            let mut h = Host::open(root).map_err(std::io::Error::other)?;
            h.app = Some(app.handle().clone());
            crate::p3::screen::start(app.handle());
            allow_assets(app.handle(), &h);
            resize_pet(app.handle(), h.store.zoom);
            #[cfg(windows)]
            if let Some(pet) = app.get_webview_window("pet") {
                h.env_settings.lock().unwrap().pet_hwnd = pet.hwnd()?.0 as isize;
            }
            // The smoke harness supplies explicit base claims. Real media/fullscreen
            // changes must not race those assertions; release builds always collect facts.
            let smoke_run =
                cfg!(feature = "smoke-test") && std::env::var_os("AYASE_SMOKE_PACKAGE").is_some();
            if !smoke_run || std::env::var_os("AYASE_SMOKE_MEDIA").is_some() {
                environment::start(h.facts.clone(), h.env_settings.clone(), h.origin);
            }
            let shared = Arc::new(Mutex::new(h));
            app.manage(shared.clone());
            hit_testing::start(app.handle());
            let app_handle = app.handle().clone();
            std::thread::spawn(move || loop {
                shared.lock().unwrap().tick();
                std::thread::sleep(Duration::from_millis(50));
            });
            use tauri::{
                menu::{Menu, MenuItem},
                tray::TrayIconBuilder,
            };
            let show = MenuItem::with_id(app, "show", "打开控制台", true, None::<&str>)?;
            let input = MenuItem::with_id(app, "input", "展开输入框", true, None::<&str>)?;
            let reset = MenuItem::with_id(app, "reset", "找回桌宠", true, None::<&str>)?;
            let exit = MenuItem::with_id(app, "exit", "退出 AyaseCompanion", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &input, &reset, &exit])?;
            TrayIconBuilder::new()
                .icon(tauri::image::Image::from_bytes(include_bytes!(
                    "../icons/icon.png"
                ))?)
                .tooltip("AyaseCompanion 0.3.0")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("control") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "input" => {
                        position_input(app);
                        if let Some(w) = app.get_webview_window("input") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "reset" => clamp_pet(app, true),
                    "exit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            clamp_pet(&app_handle, true);
            #[cfg(feature = "smoke-test")]
            if std::env::var_os("AYASE_SMOKE_P2").is_some() {
                p2::start_smoke(app.handle().clone(), app.state::<Shared>().inner().clone());
            } else {
                smoke::start(app.handle().clone(), app.state::<Shared>().inner().clone());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
            if window.label() == "pet" {
                if matches!(
                    event,
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::ScaleFactorChanged { .. }
                ) {
                    position_input(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            hit_testing::pet_hit_regions,
            p3::prepare_screen,
            p3::capability_request,
            p3::p3_action,
            p3::environment_lookup,
            p3::request_location_access,
            p3::pointer_event,
            p3::save_capabilities,
            p3::capability_probe,
            p3::local_sample_action,
            p3::open_source,
            p3::observation_page,
            p2::tools::service_probe,
            p2::tools::preview_voice,
            p2::tools::cancel_voice_preview,
            p2::tools::create_voice,
            p2::tools::refresh_voice,
            p2::save_services,
            p2::save_profile,
            p2::save_assets,
            p2::chat_request,
            p2::text_progress,
            p2::audio_segment_ack,
            p2::memory_action,
            snapshot,
            import_package,
            activate_package,
            select_instance,
            rollback_package,
            install_plugin,
            uninstall_plugin,
            save_preset,
            export_package,
            manual_request,
            presentation_ack,
            base_transition_ack,
            renderer_ready,
            user_control,
            show_window,
            reset_position,
            export_diagnostics,
            exit_app
        ])
        .run(tauri::generate_context!())
        .expect("AyaseCompanion 启动失败");
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) struct TestRoot(pub(super) PathBuf);
    impl TestRoot {
        pub(super) fn new() -> Self {
            Self(std::env::temp_dir().join(format!("ayase-host-test-{}", Uuid::new_v4())))
        }
    }
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    pub(super) fn archive(version: &str) -> Vec<u8> {
        let model = json!({"schemaVersion":"1.0","logicalSize":{"width":64,"height":64},"anchors":{"feet":{"x":0.5,"y":1},"bubble":{"x":0.5,"y":0}},"clips":{"idle":{"asset":"image","layout":{"type":"single"},"frameDurationMs":1000,"direction":"forward"}},"baseModes":{"idle":{"loop":"idle"}}});
        let files = BTreeMap::from([
            (
                "idle.png".into(),
                include_bytes!("../icons/icon.png").to_vec(),
            ),
            ("sprite.json".into(), serde_json::to_vec(&model).unwrap()),
        ]);
        let manifest = json!({"format":"ayase.character","formatVersion":"1.0","packageId":"test.identity","version":version,"displayName":"测试伙伴","avatar":{"renderer":"sprite.v1","descriptor":"sprite.json"},"assets":{"image":{"kind":"image","path":"idle.png"}}});
        package::pack(files, manifest, "manifest.json").unwrap()
    }
    #[test]
    fn stage_create_update_and_restart_preserve_identity() {
        let root = TestRoot::new();
        let mut h = Host::open(root.0.clone()).unwrap();
        let first = h.stage_archive(&archive("0.1.0")).unwrap();
        assert!(h.store.active.is_none());
        activate(&mut h, &first.hash, "new").unwrap();
        let id = h.instance().unwrap().id.clone();
        h.save_draft(&id, "保留草稿").unwrap();
        let second = h.stage_archive(&archive("0.2.0")).unwrap();
        assert_eq!(h.instance().unwrap().package_hash, first.hash);
        activate(&mut h, &second.hash, "update").unwrap();
        assert_eq!(h.instance().unwrap().id, id);
        assert_eq!(
            h.instance().unwrap().previous_packages,
            vec![first.hash.clone()]
        );
        drop(h);
        let mut h = Host::open(root.0.clone()).unwrap();
        assert_eq!(h.instance().unwrap().id, id);
        assert_eq!(h.instance().unwrap().draft, "保留草稿");
        assert!(h.scheduler.tasks.is_empty());
        activate(&mut h, &first.hash, "update").unwrap();
        assert_eq!(h.instance().unwrap().id, id);
        activate(&mut h, &first.hash, "new").unwrap();
        assert_ne!(h.instance().unwrap().id, id);
        assert_eq!(h.store.instances.len(), 2);
    }
    #[test]
    fn failed_import_or_activation_leaves_active_state_untouched() {
        let root = TestRoot::new();
        let mut h = Host::open(root.0.clone()).unwrap();
        let p = h.stage_archive(&archive("0.1.0")).unwrap();
        activate(&mut h, &p.hash, "new").unwrap();
        let before = serde_json::to_value(&h.store).unwrap();
        assert!(h.stage_archive(b"damaged ZIP").is_err());
        assert!(activate(&mut h, &"0".repeat(64), "update").is_err());
        assert_eq!(serde_json::to_value(&h.store).unwrap(), before);
        assert_eq!(h.package.as_ref().unwrap().report.hash, p.hash);
        drop(h);
        let h = Host::open(root.0.clone()).unwrap();
        assert_eq!(serde_json::to_value(&h.store).unwrap(), before);
    }
    #[test]
    fn delayed_draft_stays_with_its_original_instance() {
        let root = TestRoot::new();
        let mut h = Host::open(root.0.clone()).unwrap();
        let p = h.stage_archive(&archive("0.1.0")).unwrap();
        activate(&mut h, &p.hash, "new").unwrap();
        let old = h.instance().unwrap().id.clone();
        activate(&mut h, &p.hash, "new").unwrap();
        h.save_draft(&old, "旧伙伴的晚到草稿").unwrap();
        assert_eq!(h.instance().unwrap().draft, "");
        assert_eq!(
            h.store
                .instances
                .iter()
                .find(|i| i.id == old)
                .unwrap()
                .draft,
            "旧伙伴的晚到草稿"
        );
    }
    #[test]
    fn failed_database_commit_does_not_change_memory_state() {
        let root = TestRoot::new();
        let mut h = Host::open(root.0.clone()).unwrap();
        h.db.execute_batch("DROP TABLE settings").unwrap();
        let mut next = h.store.clone();
        next.muted = true;
        assert!(h.commit(next).is_err());
        assert!(!h.store.muted);
    }
    #[test]
    fn rename_persists_without_changing_identity_configuration_or_other_instances() {
        let root = TestRoot::new();
        let mut h = Host::open(root.0.clone()).unwrap();
        let p = h.stage_archive(&archive("0.1.0")).unwrap();
        activate(&mut h, &p.hash, "new").unwrap();
        let first = h.instance().unwrap().id.clone();
        h.save_draft(&first, "保留我的草稿").unwrap();
        let before = h.instance().unwrap().clone();
        activate(&mut h, &p.hash, "new").unwrap();
        let second = h.instance().unwrap().clone();
        h.rename_instance(&first, "  樱色 Ema  ").unwrap();
        assert_eq!(h.instance().unwrap().name, second.name);
        drop(h);
        let h = Host::open(root.0.clone()).unwrap();
        let renamed = h.store.instances.iter().find(|i| i.id == first).unwrap();
        assert_eq!(renamed.name, "樱色 Ema");
        assert_eq!(renamed.package_hash, before.package_hash);
        assert_eq!(renamed.overrides, before.overrides);
        assert_eq!(renamed.draft, before.draft);
        assert_eq!(h.store.active.as_deref(), Some(second.id.as_str()));
    }
    #[test]
    fn invalid_rename_keeps_previous_name() {
        let root = TestRoot::new();
        let mut h = Host::open(root.0.clone()).unwrap();
        let p = h.stage_archive(&archive("0.1.0")).unwrap();
        activate(&mut h, &p.hash, "new").unwrap();
        let id = h.instance().unwrap().id.clone();
        let before = h.instance().unwrap().name.clone();
        for invalid in ["  ".to_owned(), "a\nb".into(), "名".repeat(41)] {
            assert!(h.rename_instance(&id, &invalid).is_err());
        }
        assert_eq!(h.instance().unwrap().name, before);
    }
    #[test]
    fn previous_store_loads_bubble_defaults_and_custom_settings_persist() {
        let old = json!({"active":null,"instances":[],"library":[],"installed":[],"muted":false,"zoom":1.0,"excluded":[]});
        let store: Store = serde_json::from_value(old).unwrap();
        assert_eq!(store.bubble.animation, "typewriter");
        assert_eq!(store.bubble.font_size, 15);
        let root = TestRoot::new();
        let mut h = Host::open(root.0.clone()).unwrap();
        let mut next = h.store.clone();
        next.bubble.font_size = 20;
        next.bubble.characters_per_second = 50;
        h.commit(next).unwrap();
        drop(h);
        let h = Host::open(root.0.clone()).unwrap();
        assert_eq!(h.store.bubble.font_size, 20);
        assert_eq!(h.store.bubble.characters_per_second, 50);
    }
    #[test]
    fn bubble_parameters_are_bounded() {
        let mut p = BubblePreferences::default();
        assert!(p.validate().is_ok());
        p.font_size = 0;
        assert!(p.validate().is_err());
        p.font_size = 24;
        p.characters_per_second = 0;
        assert!(p.validate().is_err());
        p.characters_per_second = 80;
        p.read_ms = 0;
        assert!(p.validate().is_err());
        p.read_ms = 1000;
        p.animation = "script".into();
        assert!(p.validate().is_err());
    }
}

#[cfg(feature = "smoke-test")]
mod smoke {
    use super::*;
    fn base_restore(state: &Shared) -> Result<Value, String> {
        let start = Instant::now();
        let mut task = 0;
        let mut started = None;
        let mut paused_at = None;
        let mut idle_while_pending = false;
        let mut transition_while_text = false;
        let mut exit_outlasted_parent = false;
        let mut cases = vec![];
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let mut h = state.lock().unwrap();
            if start.elapsed() > Duration::from_secs(100) {
                return Err(format!("基础姿态收尾复验超时：{}", h.scheduler.snapshot()));
            }
            if task == 0 {
                h.scheduler
                    .claim("host.smoke-media", "music", Priority::Normal, false);
                if h.renderer_ready
                    && h.scheduler.applied_base == "music"
                    && h.scheduler.active().is_none()
                {
                    let variant = if cases.len() == 2 {
                        json!({"id":"short-base-restore","text":"短文收尾","cue":"greeting","readMs":7000})
                    } else {
                        json!({"id":"base-restore","text":"媒体已经停止，当前长文字继续阅读，恢复音乐姿态后应完整播放退出片段。".repeat(5),"audioAsset":"voice.cold","cue":"greeting","readMs":1000})
                    };
                    task = h.present(variant, "host.user", Priority::User, false, 60000, 0)?;
                }
                continue;
            }
            let t = h
                .scheduler
                .tasks
                .iter()
                .find(|t| t.id == task)
                .unwrap()
                .clone();
            let exiting = h
                .scheduler
                .base_transition
                .as_ref()
                .is_some_and(|exit| exit.parent_task_id == task);
            if exiting {
                transition_while_text |= t.channels["text"] == "pending";
                exit_outlasted_parent |= t.status.terminal();
            }
            if t.status == ayase_core::scheduler::Status::Presenting {
                let at = started.get_or_insert_with(Instant::now);
                let pause = if cases.is_empty() {
                    at.elapsed() >= Duration::from_millis(1500)
                } else {
                    t.channels["animation"] != "pending"
                };
                if paused_at.is_none() && pause {
                    h.scheduler.claims.remove("host.smoke-media");
                    paused_at = Some(h.now());
                    let now = h.now();
                    h.scheduler.record(
                        now,
                        "smoke-external-media-stopped",
                        json!({"taskId":task,"case":cases.len()+1}),
                    );
                }
                if paused_at.is_some()
                    && h.scheduler.applied_base == "idle"
                    && t.channels["text"] == "pending"
                {
                    idle_while_pending = true;
                }
            }
            if t.status.terminal() && !exiting {
                let ended = |channel: &str| {
                    h.scheduler
                        .log
                        .iter()
                        .find(|e| {
                            e["event"] == "channel-ended"
                                && e["detail"]["taskId"] == task
                                && e["detail"]["channel"] == channel
                        })
                        .and_then(|e| e["atMs"].as_u64())
                };
                let fallback = h
                    .scheduler
                    .log
                    .iter()
                    .find(|e| e["event"] == "base-fallback" && e["detail"]["taskId"] == task)
                    .and_then(|e| e["atMs"].as_u64());
                let visual = h
                    .scheduler
                    .log
                    .iter()
                    .find(|e| e["event"] == "animation-return" && e["detail"]["taskId"] == task)
                    .map(|e| e["detail"].clone())
                    .unwrap_or(Value::Null);
                let planned = h
                    .scheduler
                    .log
                    .iter()
                    .find(|e| {
                        e["event"] == "base-transition-planned"
                            && e["detail"]["parentTaskId"] == task
                    })
                    .cloned()
                    .unwrap_or(Value::Null);
                let transition_id = planned["detail"]["id"].as_u64();
                let started_at = transition_id.and_then(|id| {
                    h.scheduler
                        .log
                        .iter()
                        .find(|e| {
                            e["event"] == "base-transition-started" && e["detail"]["id"] == id
                        })
                        .and_then(|e| e["atMs"].as_u64())
                });
                let exit_end = transition_id.and_then(|id| {
                    h.scheduler
                        .log
                        .iter()
                        .find(|e| {
                            e["event"] == "base-transition-ended"
                                && e["detail"]["id"] == id
                                && e["detail"]["status"] == "completed"
                        })
                        .and_then(|e| e["atMs"].as_u64())
                });
                let rendered_count = transition_id
                    .map(|id| {
                        h.scheduler
                            .log
                            .iter()
                            .filter(|e| {
                                e["event"] == "base-transition-rendered" && e["detail"]["id"] == id
                            })
                            .count()
                    })
                    .unwrap_or(0);
                let exit_valid = transition_while_text
                    && rendered_count == 1
                    && started_at
                        .zip(exit_end)
                        .is_some_and(|(start, end)| end >= start + 1000);
                let expected_base = if cases.is_empty() { "idle" } else { "music" };
                let no_exit = !h.scheduler.tasks.iter().any(|t| {
                    t.target_base.as_deref() == Some("idle")
                        && t.accepted_at >= paused_at.unwrap_or(u64::MAX)
                });
                let timing_valid = match cases.len() {
                    0 => {
                        idle_while_pending
                            && planned.is_null()
                            && fallback
                                .zip(ended("text"))
                                .is_some_and(|(fallback, text)| fallback < text)
                    }
                    1 => {
                        idle_while_pending
                            && exit_valid
                            && exit_end
                                .zip(ended("text"))
                                .is_some_and(|(exit, text)| exit < text)
                    }
                    _ => {
                        exit_valid
                            && exit_outlasted_parent
                            && exit_end
                                .zip(ended("text"))
                                .is_some_and(|(exit, text)| exit > text)
                    }
                };
                let passed = t.status == ayase_core::scheduler::Status::Completed
                    && t.channels.values().all(|v| v == "completed")
                    && h.scheduler.applied_base == "idle"
                    && timing_valid
                    && no_exit
                    && visual["base"] == expected_base
                    && visual["kind"] == "base";
                cases.push(json!({"case":match cases.len(){0=>"stop-during-action",1=>"stop-after-action",_=>"reply-ends-before-exit"},"taskId":task,"pausedAtMs":paused_at,"fallbackAtMs":fallback,"animationEndMs":ended("animation"),"textEndMs":ended("text"),"animationReturn":visual,"idleWhileTextPending":idle_while_pending,"transitionPlanned":planned["detail"],"transitionStartedMs":started_at,"transitionEndedMs":exit_end,"transitionRenderedCount":rendered_count,"transitionWhileTextPending":transition_while_text,"exitOutlastedParent":exit_outlasted_parent,"noStaleExitTask":no_exit,"passed":passed}));
                if !passed || cases.len() == 3 {
                    return Ok(
                        json!({"result":if passed {"passed"} else {"failed"},"scope":"actual Canvas/text/Web Audio callbacks; external media claims simulated","cases":cases,"scheduler":h.scheduler.snapshot()}),
                    );
                }
                task = 0;
                started = None;
                paused_at = None;
                idle_while_pending = false;
                transition_while_text = false;
                exit_outlasted_parent = false;
            }
        }
    }
    fn self_media(state: &Shared) -> Result<Value, String> {
        let start = Instant::now();
        let variants = [
            json!({"id":"audio-last","text":"测试","audioAsset":"voice.cold","cue":"greeting","readMs":1000}),
            json!({"id":"text-last","text":"长文字继续阅读，自己的音声不应触发媒体姿态。".repeat(10),"audioAsset":"voice.cold","cue":"greeting","readMs":1000}),
            json!({"id":"audio-only","audioAsset":"voice.cold"}),
        ];
        let mut tasks = vec![];
        let mut finished = None;
        let mut samples = Vec::new();
        let mut last_sample = 0;
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let mut h = state.lock().unwrap();
            let facts = h.facts.lock().unwrap().clone();
            if facts.sample != last_sample {
                last_sample = facts.sample;
                samples.push(json!(facts));
            }
            let previous_done = tasks.last().is_none_or(|id| {
                h.scheduler
                    .tasks
                    .iter()
                    .any(|t| t.id == *id && t.status.terminal())
            });
            if tasks.len() < variants.len() && h.renderer_ready && previous_done {
                tasks.push(h.present(
                    variants[tasks.len()].clone(),
                    "host.user",
                    Priority::User,
                    false,
                    60000,
                    0,
                )?);
            }
            if tasks.len() == variants.len()
                && tasks.last().is_some_and(|id| {
                    h.scheduler
                        .tasks
                        .iter()
                        .any(|t| t.id == *id && t.status.terminal())
                })
            {
                finished.get_or_insert_with(Instant::now);
            }
            if finished.is_some_and(|at| at.elapsed() > Duration::from_secs(5))
                || start.elapsed() > Duration::from_secs(100)
            {
                let completed = tasks.len() == variants.len()
                    && tasks.iter().all(|id| {
                        h.scheduler.tasks.iter().any(|t| {
                            t.id == *id
                                && t.status == ayase_core::scheduler::Status::Completed
                                && t.channels.values().all(|v| v == "completed")
                        })
                    });
                let no_media = !samples.is_empty() && samples.iter().all(|s| s["media"] == false);
                let no_transition = !h
                    .scheduler
                    .tasks
                    .iter()
                    .any(|t| t.target_base.as_deref() == Some("music"));
                let ended = |id: u64, channel: &str| {
                    h.scheduler
                        .log
                        .iter()
                        .find(|e| {
                            e["event"] == "channel-ended"
                                && e["detail"]["taskId"] == id
                                && e["detail"]["channel"] == channel
                        })
                        .and_then(|e| e["atMs"].as_u64())
                };
                let order = tasks.len() == 3
                    && ended(tasks[0], "text")
                        .zip(ended(tasks[0], "audio"))
                        .is_some_and(|(text, audio)| text < audio)
                    && ended(tasks[1], "audio")
                        .zip(ended(tasks[1], "text"))
                        .is_some_and(|(audio, text)| audio < text);
                return Ok(
                    json!({"result":if completed && no_media && no_transition && order {"passed"} else {"failed"},"scope":"actual Web Audio and live Windows GSMTC; no other player should be active","cases":["audio-last","text-last","audio-only"],"taskIds":tasks,"channelOrderVerified":order,"completed":completed,"noExternalMedia":no_media,"noMusicTransition":no_transition,"samples":samples,"scheduler":h.scheduler.snapshot()}),
                );
            }
        }
    }
    pub fn start(app: tauri::AppHandle, state: Shared) {
        let Ok(path) = std::env::var("AYASE_SMOKE_PACKAGE") else {
            return;
        };
        std::thread::spawn(move || {
            let result = (|| -> Result<Value, String> {
                let bytes = fs::read(path).map_err(|e| e.to_string())?;
                let p = package::validate(&bytes)?;
                {
                    let mut h = state.lock().unwrap();
                    let dir = h.root.join("packages").join(&p.report.hash);
                    p.write_to(&dir.join("content"))?;
                    fs::write(dir.join("archive.zip"), &bytes).map_err(|e| e.to_string())?;
                    let mut store = h.store.clone();
                    store.library.push(p.report.clone());
                    h.commit(store)?;
                    h.startup_sent = true;
                    activate(&mut h, &p.report.hash, "new")?;
                    allow_assets(&app, &h);
                }
                if std::env::var_os("AYASE_SMOKE_MEDIA").is_some() {
                    return self_media(&state);
                }
                if std::env::var_os("AYASE_SMOKE_BASE_RESTORE").is_some() {
                    return base_restore(&state);
                }
                let start = Instant::now();
                let mut low = 0;
                let mut high = 0;
                let mut released = false;
                loop {
                    std::thread::sleep(Duration::from_millis(100));
                    let mut h = state.lock().unwrap();
                    let now = h.now();
                    if start.elapsed() > Duration::from_secs(100) {
                        return Err(format!("原生呈现闭环超时：{}", h.scheduler.snapshot()));
                    }
                    if low == 0 && h.renderer_ready {
                        low=h.present(json!({"id":"smoke-low","text":"原生窗口验证：这条文字、固定录音与招手动作会作为一个任务完成。高优先级任务与安静姿态请求将在开始后到达。","audioAsset":"voice.evening","cue":"greeting","readMs":1000}),"host.smoke-low",Priority::Background,true,60000,0)?;
                    }
                    if low > 0
                        && high == 0
                        && h.scheduler.active().is_some_and(|t| {
                            t.id == low && t.status == ayase_core::scheduler::Status::Presenting
                        })
                    {
                        high=h.present(json!({"id":"smoke-high","text":"高优先级回复在上一条完整结束后才开始。","cue":"reminder","readMs":1000}),"host.smoke-high",Priority::Important,true,90000,0)?;
                        h.scheduler.claim(
                            "host.smoke-fullscreen",
                            "quiet",
                            Priority::Important,
                            true,
                        );
                    }
                    if high > 0 && !released && h.scheduler.applied_base == "quiet" {
                        h.scheduler.claims.remove("host.smoke-fullscreen");
                        released = true;
                        h.scheduler.record(now, "smoke-release-quiet", json!(true));
                    }
                    if high > 0
                        && released
                        && h.scheduler
                            .tasks
                            .iter()
                            .any(|t| t.id == high && t.status.terminal())
                        && h.scheduler.applied_base == "idle"
                    {
                        let a = h.scheduler.tasks.iter().find(|t| t.id == low).unwrap();
                        let b = h.scheduler.tasks.iter().find(|t| t.id == high).unwrap();
                        if a.status != ayase_core::scheduler::Status::Completed
                            || b.status != ayase_core::scheduler::Status::Completed
                        {
                            return Err("原生任务未正常完成".into());
                        }
                        if a.channels.values().any(|v| v != "completed")
                            || b.channels.values().any(|v| v != "completed")
                        {
                            return Err("存在失败或未完成通道".into());
                        }
                        let low_end = h
                            .scheduler
                            .log
                            .iter()
                            .find(|e| e["event"] == "finished" && e["detail"]["taskId"] == low)
                            .unwrap()["atMs"]
                            .as_u64()
                            .unwrap();
                        let high_start = h
                            .scheduler
                            .log
                            .iter()
                            .find(|e| e["event"] == "started" && e["detail"]["taskId"] == high)
                            .unwrap()["atMs"]
                            .as_u64()
                            .unwrap();
                        if high_start < low_end {
                            return Err("违反非抢占约束".into());
                        }
                        return Ok(
                            json!({"result":"passed","scope":"actual WebView canvas/text/audio callbacks; simulated base claims","lowEndMs":low_end,"highStartMs":high_start,"scheduler":h.scheduler.snapshot(),"facts":*h.facts.lock().unwrap()}),
                        );
                    }
                }
            })();
            let h = state.lock().unwrap();
            let output = match result {
                Ok(v) => v,
                Err(e) => json!({"result":"failed","error":e,"scheduler":h.scheduler.snapshot()}),
            };
            let _ = fs::write(
                h.root.join("native-smoke.json"),
                serde_json::to_vec_pretty(&output).unwrap(),
            );
            drop(h);
            app.exit(if output["result"] == "passed" { 0 } else { 1 });
        });
    }
}
