//! Network adapters own credentials. Role packages and presentation never receive them.
use reqwest::blocking::{Client, Response};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
pub mod thinking;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Provider {
    pub enabled: bool,
    pub endpoint: String,
    pub model: String,
}
impl Default for Provider {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            model: String::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub chat_thinking: thinking::Thinking,
    pub p3: crate::p3::Settings,
    pub chat: Provider,
    pub voice: Provider,
    pub memory: Provider,
    pub voice_kind: String,
    pub max_chars: usize,
    pub timeout_seconds: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            chat_thinking: thinking::Thinking::default(),
            p3: crate::p3::Settings::default(),
            chat: Provider::default(),
            voice: Provider {
                endpoint: "https://api.fish.audio/v1".into(),
                model: "s2-pro".into(),
                ..Default::default()
            },
            memory: Provider::default(),
            voice_kind: "fish".into(),
            max_chars: 1600,
            timeout_seconds: 60,
        }
    }
}
pub fn endpoint(value: &str) -> Result<reqwest::Url, String> {
    let u = reqwest::Url::parse(value).map_err(|_| "服务地址无效")?;
    let local = u
        .host_str()
        .is_some_and(|h| h == "localhost" || h == "127.0.0.1" || h == "[::1]" || h == "::1");
    if (u.scheme() != "https" && !(u.scheme() == "http" && local))
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err("地址须为 HTTPS；本机服务可用 HTTP，不在地址中填写密钥或查询参数".into());
    }
    Ok(u)
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        self.chat_thinking.validate()?;
        self.p3.validate()?;
        for p in [&self.chat, &self.voice, &self.memory] {
            if !p.endpoint.is_empty() {
                endpoint(&p.endpoint)?;
            }
            if p.enabled && p.endpoint.is_empty() {
                return Err("启用的服务需要地址".into());
            }
            if p.model.len() > 160 || p.model.contains(['\r', '\n']) {
                return Err("模型名称无效".into());
            }
        }
        if self.chat.enabled && self.chat.model.trim().is_empty() {
            return Err("聊天服务需要模型名称".into());
        }
        if !["fish", "openai"].contains(&self.voice_kind.as_str())
            || !(1..=4000).contains(&self.max_chars)
            || !(15..=120).contains(&self.timeout_seconds)
        {
            return Err("服务选项或输出限额无效".into());
        }
        Ok(())
    }
}
pub struct Vault {
    path: PathBuf,
}
impl Vault {
    pub fn open(root: &Path) -> Result<Self, String> {
        let path = root.join("services.sqlite3");
        let db = Connection::open(&path).map_err(|_| "无法打开服务设置")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS config(id INTEGER PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS secrets(slot TEXT PRIMARY KEY, encrypted BLOB NOT NULL); CREATE TABLE IF NOT EXISTS voice_models(instance TEXT NOT NULL,endpoint TEXT NOT NULL,id TEXT NOT NULL,value TEXT NOT NULL,PRIMARY KEY(instance,endpoint,id));").map_err(|_|"无法初始化凭据存储")?;
        Ok(Self { path })
    }
    pub fn settings(&self) -> Result<Settings, String> {
        let db = Connection::open(&self.path).map_err(|_| "服务设置不可读")?;
        let s = db.query_row("SELECT value FROM config WHERE id=1", [], |r| {
            r.get::<_, String>(0)
        });
        match s {
            Ok(s) => serde_json::from_str(&s).map_err(|_| "服务配置格式错误".into()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Settings::default()),
            Err(_) => Err("服务设置不可读".into()),
        }
    }
    fn slot(kind: &str, url: &str) -> String {
        format!(
            "{kind}:{:x}",
            Sha256::digest(url.trim_end_matches('/').as_bytes())
        )
    }
    pub fn key(&self, kind: &str, url: &str) -> Result<String, String> {
        let db = Connection::open(&self.path).map_err(|_| "凭据不可读")?;
        match db.query_row(
            "SELECT encrypted FROM secrets WHERE slot=?1",
            [Self::slot(kind, url)],
            |r| r.get::<_, Vec<u8>>(0),
        ) {
            Ok(bytes) => {
                String::from_utf8(protect(&bytes, false)?).map_err(|_| "凭据不可读".into())
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(String::new()),
            Err(_) => Err("凭据不可读".into()),
        }
    }
    pub fn save(&self, s: &Settings, keys: &Value) -> Result<(), String> {
        s.validate()?;
        let mut db = Connection::open(&self.path).map_err(|_| "凭据不可写")?;
        let tx = db.transaction().map_err(|_| "凭据事务失败")?;
        for (kind, p) in [
            ("chat", &s.chat),
            ("voice", &s.voice),
            ("memory", &s.memory),
            ("vision", &s.p3.vision),
            ("search", &s.p3.search),
        ] {
            let slot = Self::slot(kind, &p.endpoint);
            if let Some(key) = keys[kind].as_str() {
                if key.len() > 4096 || key.contains(['\r', '\n']) {
                    return Err("密钥格式无效".into());
                }
                if !key.is_empty() {
                    let encrypted = protect(key.as_bytes(), true)?;
                    tx.execute("INSERT INTO secrets VALUES(?1,?2) ON CONFLICT(slot) DO UPDATE SET encrypted=excluded.encrypted",params![slot,encrypted]).map_err(|_|"凭据保存失败")?;
                }
            }
            if keys[format!("{kind}Delete")] == true {
                tx.execute("DELETE FROM secrets WHERE slot=?1", [slot])
                    .map_err(|_| "凭据清除失败")?;
            }
        }
        tx.execute(
            "INSERT INTO config VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET value=excluded.value",
            [serde_json::to_string(s).unwrap()],
        )
        .map_err(|_| "服务配置保存失败")?;
        tx.commit().map_err(|_| "服务设置保存失败".into())
    }
    pub fn public(&self, s: &Settings) -> Value {
        json!({"settings":s,"chatKeySet":self.key("chat",&s.chat.endpoint).is_ok_and(|s|!s.is_empty()),"voiceKeySet":self.key("voice",&s.voice.endpoint).is_ok_and(|s|!s.is_empty()),"memoryKeySet":self.key("memory",&s.memory.endpoint).is_ok_and(|s|!s.is_empty())})
    }
    pub fn voices(&self) -> Result<Vec<crate::voice_api::VoiceRecord>, String> {
        let db = Connection::open(&self.path).map_err(|_| "音色记录不可读")?;
        let mut stmt = db
            .prepare("SELECT value FROM voice_models ORDER BY rowid")
            .map_err(|_| "音色记录不可读")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|_| "音色记录不可读")?;
        let mut records = vec![];
        for row in rows {
            let text = row.map_err(|_| "音色记录不可读")?;
            records.push(serde_json::from_str(&text).map_err(|_| "音色记录格式错误")?);
        }
        Ok(records)
    }
    pub fn save_voice(&self, record: &crate::voice_api::VoiceRecord) -> Result<(), String> {
        let db = Connection::open(&self.path).map_err(|_| "音色记录不可写")?;
        db.execute("INSERT INTO voice_models VALUES(?1,?2,?3,?4) ON CONFLICT(instance,endpoint,id) DO UPDATE SET value=excluded.value",params![record.instance_id,record.endpoint,record.id,serde_json::to_string(record).unwrap()]).map_err(|_|"音色记录保存失败")?;
        Ok(())
    }
}
#[cfg(windows)]
fn protect(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use std::ffi::c_void;
    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }
    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *const c_void,
            prompt: *const c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *const c_void,
            prompt: *const c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(ptr: *mut c_void) -> *mut c_void;
    }
    let input = Blob {
        len: bytes.len() as u32,
        data: bytes.as_ptr() as *mut u8,
    };
    let mut output = Blob {
        len: 0,
        data: std::ptr::null_mut(),
    };
    unsafe {
        let ok = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("Windows 凭据加密失败，未保存明文".into());
        }
        let out = std::slice::from_raw_parts(output.data, output.len as usize).to_vec();
        LocalFree(output.data.cast());
        Ok(out)
    }
}
#[cfg(not(windows))]
fn protect(_: &[u8], _: bool) -> Result<Vec<u8>, String> {
    Err("此构建需要 Windows 凭据加密".into())
}
pub fn client(seconds: u64) -> Result<Client, String> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(seconds))
        .build()
        .map_err(|_| "网络服务初始化失败".into())
}
pub fn request(
    c: &Client,
    p: &Provider,
    key: &str,
    route: &str,
) -> Result<reqwest::blocking::RequestBuilder, String> {
    endpoint(&p.endpoint)?;
    let url = format!(
        "{}/{}",
        p.endpoint.trim_end_matches('/'),
        route.trim_start_matches('/')
    );
    let r = c.post(url);
    Ok(if key.is_empty() {
        r
    } else {
        r.bearer_auth(key)
    })
}
fn checked(r: Result<Response, reqwest::Error>) -> Result<Response, String> {
    let r = r.map_err(|e| {
        #[cfg(feature = "smoke-test")]
        if e.url().is_some_and(|u| u.host_str() == Some("127.0.0.1")) {
            eprintln!("isolated loopback transport: {e:?}");
        }
        if e.is_timeout() {
            "服务请求超时"
        } else {
            "服务连接失败"
        }
    })?;
    if !r.status().is_success() {
        return Err(format!("服务返回 HTTP {}", r.status().as_u16()));
    }
    Ok(r)
}
pub fn json_response(r: Result<Response, reqwest::Error>) -> Result<Value, String> {
    let mut data = vec![];
    checked(r)?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|_| "服务响应读取失败")?;
    if data.len() > 2 * 1024 * 1024 {
        return Err("服务响应过大".into());
    }
    serde_json::from_slice(&data).map_err(|_| "服务响应格式无效".into())
}

/// Preserve split delimiters across chunks and suppress reasoning, not just its field name.
#[derive(Default)]
pub struct BodyFilter {
    buffer: String,
    hidden: bool,
}
impl BodyFilter {
    pub fn feed(&mut self, s: &str, end: bool) -> String {
        self.buffer.push_str(s);
        let mut out = String::new();
        loop {
            let marker = if self.hidden { "</think>" } else { "<think>" };
            if let Some(i) = self.buffer.find(marker) {
                if !self.hidden {
                    out.push_str(&self.buffer[..i]);
                }
                self.buffer.drain(..i + marker.len());
                self.hidden = !self.hidden;
            } else {
                let keep = if end {
                    0
                } else {
                    (1..marker.len())
                        .rev()
                        .find(|&n| self.buffer.ends_with(&marker[..n]))
                        .unwrap_or(0)
                };
                let n = self.buffer.len() - keep;
                if !self.hidden {
                    out.push_str(&self.buffer[..n]);
                }
                self.buffer.drain(..n);
                break;
            }
        }
        out.chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .collect()
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEnd {
    pub finish_reason: String,
    pub output_chars: usize,
}
pub fn chat(
    s: &Settings,
    key: &str,
    messages: Vec<Value>,
    cancel: &AtomicBool,
    mut emit: impl FnMut(String) -> Result<(), String>,
) -> Result<ChatEnd, String> {
    if !s.chat.enabled {
        return Err("聊天服务尚未启用".into());
    }
    let c = client(s.timeout_seconds)?;
    let mut body = json!({"model":s.chat.model,"messages":messages,"stream":true,"max_tokens":(s.max_chars*2).max(64)});
    s.chat_thinking.apply(&s.chat, &mut body)?;
    let response = checked(
        request(&c, &s.chat, key, "chat/completions")?
            .json(&body)
            .send(),
    )?;
    let is_json = response
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h.contains("application/json"));
    let mut filter = BodyFilter::default();
    let count = std::cell::Cell::new(0usize);
    let mut output = |raw: &str, end: bool| -> Result<(), String> {
        if cancel.load(Ordering::Relaxed) {
            return Err("请求已取消".into());
        }
        let clean = filter.feed(raw, end);
        let clean: String = clean
            .chars()
            .take(s.max_chars.saturating_sub(count.get()))
            .collect();
        count.set(count.get() + clean.chars().count());
        if !clean.is_empty() {
            emit(clean)?;
        }
        Ok(())
    };
    if is_json {
        let v: Value = serde_json::from_reader(response.take(2 * 1024 * 1024))
            .map_err(|_| "聊天响应格式无效或过大")?;
        output(
            v["choices"][0]["message"]["content"]
                .as_str()
                .ok_or("聊天响应没有正文")?,
            true,
        )?;
        return Ok(ChatEnd {
            finish_reason: v["choices"][0]["finish_reason"]
                .as_str()
                .unwrap_or("unknown")
                .into(),
            output_chars: count.get(),
        });
    }
    let mut reader = BufReader::new(response);
    let mut event = String::new();
    let mut line = String::new();
    let mut ended = false;
    let mut finish_reason = String::new();
    loop {
        line.clear();
        let n = reader
            .by_ref()
            .take(262145)
            .read_line(&mut line)
            .map_err(|_| "聊天流读取失败或超时")?;
        if n == 0 {
            break;
        }
        if n > 262144 {
            return Err("聊天流单条事件过大".into());
        }
        if line.trim().is_empty() {
            if event.trim() == "[DONE]" {
                ended = true;
                break;
            }
            if !event.is_empty() {
                let v: Value = serde_json::from_str(&event).map_err(|_| "聊天流格式无效")?;
                if v.get("error").is_some() {
                    return Err("聊天流报告服务错误".into());
                }
                if let Some(content) = v["choices"][0]["delta"]["content"].as_str() {
                    output(content, false)?;
                }
                if let Some(reason) = v["choices"][0]["finish_reason"].as_str() {
                    ended = true;
                    finish_reason = reason.into();
                }
                event.clear();
            }
        } else if let Some(data) = line.strip_prefix("data:") {
            if !event.is_empty() {
                event.push('\n');
            }
            if event.len() + data.len() > 262144 {
                return Err("聊天流事件过大".into());
            }
            event.push_str(data.trim());
        }
        if count.get() >= s.max_chars {
            ended = true;
            finish_reason = "client_limit".into();
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("请求已取消".into());
        }
    }
    output("", true)?;
    if !ended {
        return Err("聊天流意外断开".into());
    }
    Ok(ChatEnd {
        finish_reason: if finish_reason.is_empty() {
            "done".into()
        } else {
            finish_reason
        },
        output_chars: count.get(),
    })
}
pub fn speech(s: &Settings, key: &str, text: &str, voice: &str) -> Result<Vec<u8>, String> {
    if !s.voice.enabled {
        return Err("语音服务未启用".into());
    }
    if voice.is_empty() {
        return Err("当前伙伴尚未绑定音色".into());
    }
    let c = client(s.timeout_seconds)?;
    let r = if s.voice_kind == "fish" {
        request(&c,&s.voice,key,"tts")?.header("model",&s.voice.model).json(&json!({"text":text,"reference_id":voice,"format":"mp3","latency":"normal","prosody":{"speed":1.0}}))
    } else {
        request(&c, &s.voice, key, "audio/speech")?.json(
            &json!({"model":s.voice.model,"input":text,"voice":voice,"response_format":"mp3"}),
        )
    };
    let mut data = vec![];
    checked(r.send())?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|_| "语音响应读取失败")?;
    if data.is_empty() || data.len() > 16 * 1024 * 1024 {
        return Err("语音数据为空或过大".into());
    }
    Ok(data)
}
pub fn cancelled() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hidden_split_reasoning() {
        let mut f = BodyFilter::default();
        assert_eq!(f.feed("<thi", false), "");
        assert_eq!(f.feed("nk>秘密</th", false), "");
        assert_eq!(f.feed("ink>你好\u{0}", true), "你好");
    }
    #[test]
    fn endpoints_cannot_smuggle_credentials() {
        assert!(endpoint("https://example.invalid/v1").is_ok());
        assert!(endpoint("http://127.0.0.1:8888").is_ok());
        for s in [
            "http://example.invalid",
            "https://u:p@example.invalid",
            "https://example.invalid?key=x",
            "file:///tmp/a",
        ] {
            assert!(endpoint(s).is_err());
        }
    }
    #[test]
    #[cfg(windows)]
    fn credential_ciphertext_round_trip() {
        let value = b"synthetic-credential-for-unit-test";
        let blob = protect(value, true).unwrap();
        assert!(!blob.windows(value.len()).any(|w| w == value));
        assert_eq!(protect(&blob, false).unwrap(), value);
    }
    #[test]
    #[cfg(windows)]
    fn credentials_are_slot_and_endpoint_scoped_and_can_be_cleared() {
        let root = std::env::temp_dir().join(format!("ayase-vault-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let vault = Vault::open(&root).unwrap();
        let mut settings = Settings::default();
        settings.chat.endpoint = "https://chat.example.invalid/v1".into();
        settings.voice.endpoint = settings.chat.endpoint.clone();
        vault
            .save(
                &settings,
                &json!({"chat":"synthetic-chat-secret","voice":"synthetic-voice-secret"}),
            )
            .unwrap();
        assert_eq!(
            vault.key("chat", &settings.chat.endpoint).unwrap(),
            "synthetic-chat-secret"
        );
        assert_eq!(
            vault.key("voice", &settings.voice.endpoint).unwrap(),
            "synthetic-voice-secret"
        );
        assert!(vault
            .key("chat", "https://other.example.invalid/v1")
            .unwrap()
            .is_empty());
        vault
            .save(&settings, &json!({"chat":"","chatDelete":true}))
            .unwrap();
        assert!(vault
            .key("chat", &settings.chat.endpoint)
            .unwrap()
            .is_empty());
        assert!(!vault
            .key("voice", &settings.voice.endpoint)
            .unwrap()
            .is_empty());
        drop(vault);
        std::fs::remove_dir_all(root).unwrap();
    }
}
