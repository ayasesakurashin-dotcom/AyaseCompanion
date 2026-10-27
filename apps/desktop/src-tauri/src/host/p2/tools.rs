use super::*;
use crate::voice_api::{self, Sample, VoiceRecord};
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct Tools {
    pub busy: BTreeSet<String>,
    pub previews: BTreeMap<u64, PathBuf>,
    pub voices: BTreeMap<String, Vec<VoiceRecord>>,
    pub samples: Vec<Value>,
    pub sample_scope: String,
    revision: String,
    next_refresh: u64,
}
impl Tools {
    pub fn open(vault: &services::Vault) -> Result<Self, String> {
        let mut s = Self::default();
        for record in vault.voices()? {
            s.remember(record);
        }
        Ok(s)
    }
    fn remember(&mut self, record: VoiceRecord) {
        let list = self.voices.entry(record.instance_id.clone()).or_default();
        list.retain(|r| r.id != record.id || r.endpoint != record.endpoint);
        list.push(record);
    }
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::host) struct Connection {
    pub provider: services::Provider,
    pub voice_kind: String,
    pub timeout_seconds: u64,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub clear_key: bool,
}
impl Connection {
    fn settings(&self, kind: &str) -> Result<services::Settings, String> {
        let mut s = services::Settings::default();
        s.voice_kind = self.voice_kind.clone();
        s.timeout_seconds = self.timeout_seconds;
        let mut p = self.provider.clone();
        p.enabled = true;
        match kind {
            "chat" => s.chat = p,
            "voice" => s.voice = p,
            "memory" => s.memory = p,
            _ => return Err("未知服务".into()),
        }
        s.validate()?;
        Ok(s)
    }
    fn key(&self, h: &Host, kind: &str) -> Result<String, String> {
        if self.key.len() > 4096 || self.key.contains(['\r', '\n']) {
            return Err("密钥格式无效".into());
        }
        if self.clear_key {
            return Ok(String::new());
        }
        if !self.key.is_empty() {
            return Ok(self.key.clone());
        }
        h.ai.vault.key(kind, &self.provider.endpoint)
    }
}
struct Lease {
    state: Shared,
    slot: String,
}
impl Lease {
    fn take(h: &mut Host, state: &Shared, slot: &str) -> Result<Self, String> {
        if !h.ai.tools.busy.insert(slot.into()) {
            return Err("该操作正在进行，请稍候".into());
        }
        Ok(Self {
            state: state.clone(),
            slot: slot.into(),
        })
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut h) = self.state.lock() {
            h.ai.tools.busy.remove(&self.slot);
        }
    }
}
fn transcript(value: &Value, id: &str, p: &Package) -> Option<String> {
    if value["audioAsset"] == id {
        if let Some(s) = value["text"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| value["textAsset"].as_str().and_then(|id| p.asset_text(id)))
        {
            return Some(s.chars().take(2000).collect());
        }
    }
    match value {
        Value::Object(o) => o.values().find_map(|v| transcript(v, id, p)),
        Value::Array(a) => a.iter().find_map(|v| transcript(v, id, p)),
        _ => None,
    }
}
pub(super) fn tick(h: &mut Host) {
    let done =
        h.ai.tools
            .previews
            .keys()
            .copied()
            .filter(|id| {
                h.scheduler
                    .tasks
                    .iter()
                    .find(|t| t.id == *id)
                    .is_none_or(|t| t.status.terminal())
            })
            .collect::<Vec<_>>();
    for id in done {
        if let Some(path) = h.ai.tools.previews.remove(&id) {
            if path.parent() == Some(h.root.join("audio-cache").as_path()) {
                let _ = fs::remove_dir_all(path);
            }
        }
    }
    let scope = h
        .instance()
        .map(|i| format!("{}:{}", i.id, i.package_hash))
        .unwrap_or_default();
    let now = h.now();
    if scope == h.ai.tools.sample_scope && now < h.ai.tools.next_refresh {
        return;
    }
    h.ai.tools.next_refresh = now + 1000;
    let revision = format!("{}:{:?}", scope, h.instance().map(|i| &i.revisions));
    if revision == h.ai.tools.revision {
        return;
    }
    h.ai.tools.revision = revision;
    h.ai.tools.sample_scope = scope;
    let samples = if let Some(p) = &h.package {
        let configs = h
            .configured()
            .into_iter()
            .map(|c| c.config)
            .collect::<Vec<_>>();
        p.manifest["assets"].as_object().into_iter().flatten().filter(|(_,v)|v["kind"]=="audio"&&v["path"].as_str().is_some_and(|path|p.files.contains_key(path))).map(|(id,v)|json!({"id":id,"durationMs":v["durationMs"],"transcript":configs.iter().find_map(|c|transcript(c,id,p)).unwrap_or_default(),"enabled":h.asset_enabled(id)})).collect::<Vec<_>>()
    } else {
        vec![]
    };
    h.ai.tools.samples = samples;
}
fn require_owner(h: &Host, id: &str) -> Result<(), String> {
    if h.store.active.as_deref() != Some(id) {
        return Err("伙伴已切换，请重新确认操作对象".into());
    }
    Ok(())
}
pub(super) fn stop_previews(h: &mut Host, id: &str) {
    let now = h.now();
    let tasks =
        h.ai.tools
            .previews
            .keys()
            .copied()
            .filter(|task| {
                h.scheduler
                    .tasks
                    .iter()
                    .any(|t| t.id == *task && t.instance_id == id && !t.status.terminal())
            })
            .collect::<Vec<_>>();
    for task in tasks {
        h.scheduler
            .finish(task, Status::Cancelled, "用户停止试听", now);
    }
}

#[tauri::command]
pub(in crate::host) async fn service_probe(
    window: WebviewWindow,
    state: State<'_, Shared>,
    kind: String,
    connection: Connection,
) -> Result<Value, String> {
    control(&window)?;
    let shared = state.inner().clone();
    let (s, key, lease) = {
        let mut h = shared.lock().unwrap();
        let s = connection.settings(&kind)?;
        let key = connection.key(&h, &kind)?;
        let lease = Lease::take(&mut h, &shared, &format!("probe:{kind}"))?;
        (s, key, lease)
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        let start = Instant::now();
        let message = voice_api::probe(&kind, &s, &key)?;
        Ok(json!({"message":message,"elapsedMs":start.elapsed().as_millis()}))
    })
    .await
    .map_err(|_| "连接测试未能完成".to_owned())?
}
#[tauri::command]
pub(in crate::host) async fn preview_voice(
    window: WebviewWindow,
    state: State<'_, Shared>,
    instance_id: String,
    text: String,
    voice_id: String,
    connection: Connection,
) -> Result<Value, String> {
    control(&window)?;
    if text.trim().is_empty() || text.chars().count() > 200 {
        return Err("试听文字限1—200字".into());
    }
    if voice_id.trim().is_empty() || voice_id.len() > 256 || voice_id.contains(['\r', '\n']) {
        return Err("请先填写或选择有效音色 ID".into());
    }
    let shared = state.inner().clone();
    let (s, key, id, generation, directory, lease) = {
        let mut h = shared.lock().unwrap();
        require_owner(&h, &instance_id)?;
        h.package.as_ref().ok_or("请先选择伙伴")?;
        let s = connection.settings("voice")?;
        let key = connection.key(&h, "voice")?;
        if h.ai.tools.busy.contains("preview") {
            return Err("试听正在准备，请稍候或先停止试听".into());
        }
        stop_previews(&mut h, &instance_id);
        let (id, generation, directory) = prepare_preview(&mut h, &instance_id, s.timeout_seconds)?;
        let lease = Lease::take(&mut h, &shared, "preview")?;
        (s, key, id, generation, directory, lease)
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        let result = services::speech(&s, &key, &text, &voice_id);
        let mut h = shared.lock().unwrap();
        complete_preview(&mut h, &instance_id, id, generation, &directory, result)
    })
    .await
    .map_err(|_| "试听操作未能完成".to_owned())?
}
#[tauri::command]
pub(in crate::host) fn cancel_voice_preview(
    window: WebviewWindow,
    state: State<Shared>,
    instance_id: String,
) -> Result<(), String> {
    control(&window)?;
    let mut h = state.lock().unwrap();
    require_owner(&h, &instance_id)?;
    stop_previews(&mut h, &instance_id);
    Ok(())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::host) struct SampleSelection {
    asset_id: String,
    #[serde(default)]
    transcript: String,
}
#[tauri::command]
pub(in crate::host) async fn create_voice(
    window: WebviewWindow,
    state: State<'_, Shared>,
    instance_id: String,
    package_hash: String,
    title: String,
    samples: Vec<SampleSelection>,
    connection: Connection,
) -> Result<VoiceRecord, String> {
    control(&window)?;
    let shared = state.inner().clone();
    let (s, key, selected, lease) = {
        let mut h = shared.lock().unwrap();
        require_owner(&h, &instance_id)?;
        let s = connection.settings("voice")?;
        if s.voice_kind != "fish" {
            return Err("请将语音接口设为 Fish Audio".into());
        }
        let key = connection.key(&h, "voice")?;
        if h.ai
            .tools
            .voices
            .get(&instance_id)
            .is_some_and(|v| v.len() >= 100)
        {
            return Err("当前伙伴已保存100条音色记录".into());
        }
        let selected = prepare_samples(&h, &instance_id, &package_hash, samples)?;
        let lease = Lease::take(&mut h, &shared, "create-voice")?;
        (s, key, selected, lease)
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        let record = voice_api::create(&s, &key, &instance_id, &title, selected)?;
        {
            let mut h = shared.lock().unwrap();
            store_voice(&mut h, record.clone())?;
        }
        Ok(record)
    })
    .await
    .map_err(|_| "音色创建未能完成；请先在服务控制台核对，避免重复创建".to_owned())?
}
#[tauri::command]
pub(in crate::host) async fn refresh_voice(
    window: WebviewWindow,
    state: State<'_, Shared>,
    instance_id: String,
    id: String,
    source_endpoint: String,
    connection: Connection,
) -> Result<VoiceRecord, String> {
    control(&window)?;
    let shared = state.inner().clone();
    let (s, key, record, lease) = {
        let mut h = shared.lock().unwrap();
        require_owner(&h, &instance_id)?;
        let record =
            h.ai.tools
                .voices
                .get(&instance_id)
                .into_iter()
                .flatten()
                .find(|r| r.id == id && r.endpoint == source_endpoint)
                .cloned()
                .ok_or("此音色不属于该伙伴的创建记录")?;
        let s = connection.settings("voice")?;
        let key = connection.key(&h, "voice")?;
        let lease = Lease::take(&mut h, &shared, "refresh-voice")?;
        (s, key, record, lease)
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _lease = lease;
        let record = voice_api::refresh(&s, &key, &record)?;
        {
            let mut h = shared.lock().unwrap();
            store_voice(&mut h, record.clone())?;
        }
        Ok(record)
    })
    .await
    .map_err(|_| "音色状态查询未能完成".to_owned())?
}

pub(super) fn prepare_preview(
    h: &mut Host,
    owner: &str,
    timeout: u64,
) -> Result<(u64, u64, PathBuf), String> {
    require_owner(h, owner)?;
    let now = h.now();
    let id = h.scheduler.submit(
        Request {
            instance_id: owner.into(),
            owner: "host.user".into(),
            revision: 0,
            priority: Priority::User,
            proactive: false,
            ttl_ms: 300000,
            preparing: true,
            plan: json!({"voicePreview":true}),
            channels: vec!["audio".into()],
            dedupe: None,
        },
        now,
    )?;
    let task = h.scheduler.tasks.iter_mut().find(|t| t.id == id).unwrap();
    task.prepare_deadline = now + (timeout + 5) * 1000;
    let generation = task.generation;
    let directory = h
        .root
        .join("audio-cache")
        .join(format!("preview-{}", Uuid::new_v4()));
    h.ai.tools.previews.insert(id, directory.clone());
    Ok((id, generation, directory))
}
pub(super) fn complete_preview(
    h: &mut Host,
    owner: &str,
    id: u64,
    generation: u64,
    directory: &std::path::Path,
    result: Result<Vec<u8>, String>,
) -> Result<Value, String> {
    let valid = h
        .scheduler
        .tasks
        .iter()
        .any(|t| t.id == id && t.generation == generation && t.status == Status::Preparing)
        && h.store.active.as_deref() == Some(owner);
    if !valid {
        return Ok(
            json!({"taskId":id,"cancelled":true,"message":"试听已停止或伙伴已切换，未播放迟到音声。"}),
        );
    }
    let now = h.now();
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(e) => {
            h.scheduler.finish(id, Status::Failed, "试听合成失败", now);
            return Err(e);
        }
    };
    let path = directory.join("preview.mp3");
    if fs::create_dir_all(directory)
        .and_then(|_| fs::write(&path, bytes))
        .is_err()
    {
        h.scheduler
            .finish(id, Status::Failed, "试听缓存不可写", now);
        return Err("试听缓存不可写".into());
    }
    if !h.scheduler.ready(
        id,
        generation,
        json!({"voicePreview":true,"audioPath":path.to_string_lossy(),"audioDurationMs":60000}),
        vec!["audio".into()],
        now,
    ) {
        h.scheduler
            .finish(id, Status::Failed, "试听准备已超时", now);
        return Err("试听准备已超时".into());
    }
    Ok(json!({"taskId":id,"message":"试听已加入音声队列，当前回复完整结束后播放。"}))
}
fn prepare_samples(
    h: &Host,
    owner: &str,
    hash: &str,
    samples: Vec<SampleSelection>,
) -> Result<Vec<Sample>, String> {
    require_owner(h, owner)?;
    let p = h
        .package
        .as_ref()
        .filter(|p| p.report.hash == hash)
        .ok_or("角色包已变化，请重新选择音声素材")?;
    if samples.is_empty() || samples.len() > 5 {
        return Err("请选择1—5段音声素材".into());
    }
    let mut ids = BTreeSet::new();
    let mut selected = vec![];
    let mut total = 0;
    for sample in samples {
        if !ids.insert(sample.asset_id.clone()) {
            return Err("请勿重复选择相同素材".into());
        }
        let asset = &p.manifest["assets"][&sample.asset_id];
        if asset["kind"] != "audio" {
            return Err("只允许使用当前角色包中的固定音声".into());
        }
        let path = asset["path"].as_str().ok_or("音声资源无效")?;
        let bytes = p.files.get(path).ok_or("音声资源不存在")?;
        total += bytes.len();
        if bytes.len() > 10 * 1024 * 1024 || total > 25 * 1024 * 1024 {
            return Err("音声素材超过上传限额：单段10 MiB、总计25 MiB".into());
        }
        selected.push(Sample {
            bytes: bytes.clone(),
            extension: std::path::Path::new(path)
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .into(),
            transcript: sample.transcript,
        });
    }
    Ok(selected)
}
fn store_voice(h: &mut Host, record: VoiceRecord) -> Result<(), String> {
    h.ai.vault.save_voice(&record)?;
    h.ai.tools.remember(record);
    Ok(())
}
#[cfg(test)]
mod tests;
