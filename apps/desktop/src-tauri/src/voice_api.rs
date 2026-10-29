//! Explicitly invoked account probes and Fish voice creation. No role history is sent.
use crate::services::{self, Provider, Settings};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceRecord {
    pub instance_id: String,
    pub endpoint: String,
    pub id: String,
    pub title: String,
    pub state: String,
    pub visibility: String,
}
pub struct Sample {
    pub bytes: Vec<u8>,
    pub extension: String,
    pub transcript: String,
}
pub fn fish_model_url(p: &Provider, id: Option<&str>) -> Result<reqwest::Url, String> {
    let mut url = services::endpoint(&p.endpoint)?;
    let prefix = url
        .path()
        .trim_end_matches('/')
        .strip_suffix("/v1")
        .unwrap_or(url.path().trim_end_matches('/'))
        .to_owned();
    url.set_path(&format!("{prefix}/model"));
    if let Some(id) = id {
        if id.is_empty()
            || id.len() > 128
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err("音色 ID 格式无效".into());
        }
        url.path_segments_mut()
            .map_err(|_| "模型服务地址无效")?
            .push(id);
    }
    Ok(url)
}
pub fn probe(kind: &str, s: &Settings, key: &str) -> Result<String, String> {
    let c = services::client(s.timeout_seconds.min(30))?;
    let p = match kind {
        "chat" => &s.chat,
        "voice" => &s.voice,
        "memory" => &s.memory,
        _ => return Err("未知服务类型".into()),
    };
    services::endpoint(&p.endpoint)?;
    match kind {
        "chat" => {
            let mut settings = s.clone();
            settings.chat.enabled = true;
            settings.timeout_seconds = settings.timeout_seconds.min(30);
            settings.max_chars = 200;
            if settings.chat.model.trim().is_empty() {
                return Err("请先填写聊天模型名称".into());
            }
            let mut text = String::new();
            services::chat(
                &settings,
                key,
                vec![json!({"role":"user","content":"This is a connection test. Reply only OK."})],
                &AtomicBool::new(false),
                |s| {
                    text.push_str(&s);
                    Ok(())
                },
            )?;
            if text.trim().is_empty() {
                return Err("服务已响应，但未返回可用正文；请检查模型与接口兼容性".into());
            }
            Ok("聊天连接成功，已收到模型正文；没有写入对话或记忆。".into())
        }
        "voice" => {
            let mut url = if s.voice_kind == "fish" {
                fish_model_url(p, None)?
            } else {
                services::endpoint(&format!("{}/models", p.endpoint.trim_end_matches('/')))?
            };
            if s.voice_kind == "fish" {
                url.query_pairs_mut()
                    .append_pair("page_size", "1")
                    .append_pair("self", "true");
            }
            let r = c.get(url);
            let r = if key.is_empty() {
                r
            } else {
                r.bearer_auth(key)
            };
            let value = services::json_response(r.send())?;
            if !value["items"].is_array() && !value["data"].is_array() {
                return Err(
                    "服务已响应，但不是预期的音色或模型接口；可用下方试听验证合成接口".into(),
                );
            }
            Ok("语音账户接口可访问；请选择音色并点击“合成并试听”验证实际声音。".into())
        }
        "memory" => {
            let r = c.get(format!(
                "{}/v1/default/banks?limit=1",
                p.endpoint.trim_end_matches('/')
            ));
            let r = if key.is_empty() {
                r
            } else {
                r.bearer_auth(key)
            };
            let value = services::json_response(r.send())?;
            if !value["banks"].is_array() {
                return Err("服务响应不是 Hindsight 管理接口".into());
            }
            Ok("Hindsight 管理接口可访问；未创建、写入或删除任何记忆。未开启服务端认证时，此结果只验证连接。".into())
        }
        _ => unreachable!(),
    }
}
pub fn parse_voice(
    value: &Value,
    instance: &str,
    endpoint: &str,
    title: &str,
) -> Result<VoiceRecord, String> {
    let id = value["_id"]
        .as_str()
        .or_else(|| value["id"].as_str())
        .ok_or("服务未返回音色 ID，请先到服务控制台核对，避免重复创建")?;
    fish_model_url(
        &Provider {
            endpoint: endpoint.into(),
            ..Default::default()
        },
        Some(id),
    )?;
    Ok(VoiceRecord {
        instance_id: instance.into(),
        endpoint: endpoint.into(),
        id: id.into(),
        title: title.chars().take(80).collect(),
        state: value["state"]
            .as_str()
            .unwrap_or("unknown")
            .chars()
            .take(48)
            .collect(),
        visibility: value["visibility"]
            .as_str()
            .unwrap_or("unknown")
            .chars()
            .take(24)
            .collect(),
    })
}
pub fn create(
    s: &Settings,
    key: &str,
    instance: &str,
    title: &str,
    samples: Vec<Sample>,
) -> Result<VoiceRecord, String> {
    if s.voice_kind != "fish" {
        return Err("素材创建音色需要选择 Fish Audio 接口".into());
    }
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 80 || title.chars().any(char::is_control) {
        return Err("请填写1—80字的音色名称".into());
    }
    if samples.is_empty() || samples.len() > 5 {
        return Err("请选择1—5段固定音声素材".into());
    }
    if samples
        .iter()
        .any(|s| s.bytes.is_empty() || s.bytes.len() > 10 * 1024 * 1024)
        || samples.iter().map(|s| s.bytes.len()).sum::<usize>() > 25 * 1024 * 1024
    {
        return Err("单段素材上限10 MiB，总上传上限25 MiB".into());
    }
    let with_text = samples.iter().all(|s| !s.transcript.trim().is_empty());
    if !with_text && samples.iter().any(|s| !s.transcript.trim().is_empty()) {
        return Err("请为每段填写对应台词，或全部留空由服务识别".into());
    }
    let mut form = reqwest::blocking::multipart::Form::new()
        .text("type", "tts")
        .text("title", title.to_owned())
        .text("train_mode", "fast")
        .text("visibility", "private")
        .text("generate_sample", "false");
    for (index, sample) in samples.into_iter().enumerate() {
        if sample.transcript.chars().count() > 2000 {
            return Err("单段对应台词过长".into());
        }
        let extension = sample.extension.to_ascii_lowercase();
        let mime = match extension.as_str() {
            "wav" => "audio/wav",
            "mp3" => "audio/mpeg",
            "flac" => "audio/flac",
            "ogg" | "opus" => "audio/ogg",
            "m4a" => "audio/mp4",
            "webm" => "audio/webm",
            _ => return Err("此素材音频格式不支持上传".into()),
        };
        let part = reqwest::blocking::multipart::Part::bytes(sample.bytes)
            .file_name(format!("sample-{}.{}", index + 1, extension))
            .mime_str(mime)
            .map_err(|_| "音频格式无效")?;
        form = form.part("voices", part);
        if with_text {
            form = form.text("texts", sample.transcript);
        }
    }
    let c = services::client(s.timeout_seconds)?;
    let r = c.post(fish_model_url(&s.voice, None)?).multipart(form);
    let r = if key.is_empty() {
        r
    } else {
        r.bearer_auth(key)
    };
    let value = services::json_response(r.send())?;
    parse_voice(&value, instance, &s.voice.endpoint, title)
}
pub fn refresh(s: &Settings, key: &str, record: &VoiceRecord) -> Result<VoiceRecord, String> {
    if s.voice_kind != "fish" {
        return Err("查询 Fish 音色需要选择 Fish Audio 接口".into());
    }
    if s.voice.endpoint.trim_end_matches('/') != record.endpoint.trim_end_matches('/') {
        return Err("当前语音地址与创建时不同，请先恢复该音色的服务地址再查询".into());
    }
    let c = services::client(s.timeout_seconds.min(30))?;
    let r = c.get(fish_model_url(&s.voice, Some(&record.id))?);
    let r = if key.is_empty() {
        r
    } else {
        r.bearer_auth(key)
    };
    let value = services::json_response(r.send())?;
    parse_voice(&value, &record.instance_id, &record.endpoint, &record.title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::Server;
    fn settings(server: &Server) -> Settings {
        let mut s = Settings::default();
        let p = Provider {
            enabled: true,
            endpoint: format!("{}/v1", server.endpoint),
            model: "fixture".into(),
        };
        s.chat = p.clone();
        s.voice = p;
        s.memory = Provider {
            enabled: true,
            endpoint: server.endpoint.clone(),
            model: String::new(),
        };
        s
    }
    fn sample(text: &str) -> Sample {
        Sample {
            bytes: b"synthetic-selected-audio".to_vec(),
            extension: "wav".into(),
            transcript: text.into(),
        }
    }
    #[test]
    fn probes_use_each_account_route_and_memory_probe_is_read_only() {
        let server = Server::start();
        let mut s = settings(&server);
        for kind in ["chat", "voice", "memory"] {
            assert!(probe(kind, &s, "synthetic-key").is_ok());
        }
        s.voice_kind = "openai".into();
        assert!(probe("voice", &s, "").is_ok());
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[0]["body"]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("connection test"));
        assert_eq!(requests[0]["body"]["messages"].as_array().unwrap().len(), 1);
        assert!(requests[1]["path"]
            .as_str()
            .unwrap()
            .starts_with("GET /model?"));
        assert!(requests[2]["path"]
            .as_str()
            .unwrap()
            .starts_with("GET /v1/default/banks?limit=1 "));
        assert!(requests[3]["path"]
            .as_str()
            .unwrap()
            .starts_with("GET /v1/models "));
    }
    #[test]
    fn failed_probe_reports_status_without_server_body_or_key() {
        let server = Server::start();
        let s = settings(&server);
        for kind in ["chat", "voice", "memory"] {
            let e = probe(kind, &s, "synthetic-invalid-key").unwrap_err();
            assert!(e.contains("401"), "{kind}: {e}");
            assert!(!e.contains("synthetic"));
            assert!(!e.contains("do-not-display"));
        }
    }
    #[test]
    fn model_endpoint_preserves_proxy_prefix_and_rejects_path_ids() {
        for (base, expected) in [
            ("https://api.fish.audio/v1/", "https://api.fish.audio/model"),
            (
                "https://example.org/proxy/v1",
                "https://example.org/proxy/model",
            ),
            ("https://example.org", "https://example.org/model"),
        ] {
            let p = Provider {
                endpoint: base.into(),
                ..Default::default()
            };
            assert_eq!(fish_model_url(&p, None).unwrap().as_str(), expected);
            assert!(fish_model_url(&p, Some("../other")).is_err());
        }
    }
    #[test]
    fn multipart_upload_is_private_has_selected_bytes_and_returns_refreshable_id() {
        let server = Server::start();
        let s = settings(&server);
        let record = create(
            &s,
            "synthetic-key",
            "instance-a",
            "My voice",
            vec![sample("corresponding transcript")],
        )
        .unwrap();
        assert_eq!(record.id, "fixture_voice_1");
        assert_eq!(record.instance_id, "instance-a");
        assert_eq!(record.visibility, "private");
        assert_eq!(
            refresh(&s, "synthetic-key", &record).unwrap().state,
            "trained"
        );
        let requests = server.requests.lock().unwrap();
        let raw = requests[0]["raw"].as_str().unwrap();
        for expected in [
            "name=\"visibility\"\r\n\r\nprivate",
            "name=\"train_mode\"\r\n\r\nfast",
            "name=\"voices\"; filename=\"sample-1.wav\"",
            "synthetic-selected-audio",
            "name=\"texts\"\r\n\r\ncorresponding transcript",
        ] {
            assert!(raw.contains(expected), "missing multipart field {expected}");
        }
        assert!(!raw.contains("instance-a"));
        let mut changed = s.clone();
        changed.voice.endpoint = "https://example.org/v1".into();
        assert!(refresh(&changed, "", &record).is_err());
    }
    #[test]
    fn invalid_upload_is_rejected_before_network() {
        let server = Server::start();
        let s = settings(&server);
        assert!(create(&s, "", "a", "test", vec![]).is_err());
        assert!(create(&s, "", "a", "test", vec![sample("one"), sample("")]).is_err());
        let mut invalid = sample("");
        invalid.extension = "exe".into();
        assert!(create(&s, "", "a", "test", vec![invalid]).is_err());
        assert!(server.requests.lock().unwrap().is_empty());
    }
}
