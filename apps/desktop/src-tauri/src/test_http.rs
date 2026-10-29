//! Deterministic HTTP fixture, compiled only for tests and isolated smoke builds.
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
pub struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
}
impl Server {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(vec![]));
        let stopped = stop.clone();
        let captured = requests.clone();
        thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let captured = captured.clone();
                        thread::spawn(move || {
                            let _ = respond(stream, captured);
                        });
                    }
                    Err(_) => thread::sleep(Duration::from_millis(10)),
                }
            }
        });
        Self {
            endpoint,
            requests,
            stop,
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
fn response(stream: &mut TcpStream, status: &str, kind: &str, data: &[u8]) -> std::io::Result<()> {
    write!(stream,"HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",data.len())?;
    stream.write_all(data)?;
    stream.flush()?;
    // Half-close after the complete body; let the peer consume it before dropping
    // the socket. Immediate close can abort a loopback response on Windows.
    stream.shutdown(std::net::Shutdown::Write)?;
    let _ = stream.read(&mut [0u8; 1]);
    Ok(())
}
fn respond(mut stream: TcpStream, requests: Arc<Mutex<Vec<Value>>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut bytes = vec![];
    let header_end = loop {
        let mut b = [0u8; 2048];
        let n = stream.read(&mut b)?;
        if n == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&b[..n]);
        if let Some(i) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            break i + 4;
        }
        if bytes.len() > 16384 {
            return Ok(());
        }
    };
    let header = String::from_utf8_lossy(&bytes[..header_end]).to_string();
    let length = header
        .lines()
        .find_map(|line| {
            line.to_lowercase()
                .strip_prefix("content-length:")
                .and_then(|s| s.trim().parse::<usize>().ok())
        })
        .unwrap_or(0)
        .min(128000);
    while bytes.len() < header_end + length {
        let mut b = [0u8; 4096];
        let n = stream.read(&mut b)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&b[..n]);
    }
    let raw = String::from_utf8_lossy(&bytes[header_end..]).to_string();
    let body: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    let route = header.lines().next().unwrap_or("");
    let auth = header
        .lines()
        .find(|l| l.to_lowercase().starts_with("authorization:"))
        .unwrap_or("");
    requests
        .lock()
        .unwrap()
        .push(json!({"body":body,"raw":raw,"path":route,"authorization":auth}));
    if auth.contains("synthetic-invalid-key") {
        return response(
            &mut stream,
            "401 Unauthorized",
            "application/json",
            b"{\"error\":\"do-not-display-server-body\"}",
        );
    }
    if route.starts_with("POST /model ") || route.starts_with("POST /proxy/model ") {
        return response(
            &mut stream,
            "200 OK",
            "application/json",
            br#"{"_id":"fixture_voice_1","state":"created","visibility":"private"}"#,
        );
    }
    if route.starts_with("GET /model/fixture_voice_1 ") {
        return response(
            &mut stream,
            "200 OK",
            "application/json",
            br#"{"_id":"fixture_voice_1","state":"trained","visibility":"private"}"#,
        );
    }
    if route.starts_with("GET /model?") || route.starts_with("GET /v1/models ") {
        return response(
            &mut stream,
            "200 OK",
            "application/json",
            br#"{"items":[],"data":[]}"#,
        );
    }
    if route.starts_with("GET /v1/default/banks?limit=1 ") {
        return response(
            &mut stream,
            "200 OK",
            "application/json",
            br#"{"banks":[]}"#,
        );
    }
    if header
        .lines()
        .next()
        .unwrap_or("")
        .contains("chat/completions")
    {
        let input = body["messages"]
            .as_array()
            .and_then(|a| a.last())
            .and_then(|v| v["content"].as_str())
            .unwrap_or("");
        if input.contains("[slow]") {
            thread::sleep(Duration::from_millis(600));
        }
        if input.contains("[fail]") {
            return response(
                &mut stream,
                "503 Service Unavailable",
                "application/json",
                b"{\"error\":\"synthetic\"}",
            );
        }
        let long = input.contains("[long]");
        let chunks = if input.contains("[markdown]") {
            vec!["# 合成测试摘要\n\n第一句是**正常正文**，含 emoji 🙂。\n\n1. 第一项内容。\n2. 第二项内容。"]
        } else if input.contains("[tail-slow]") {
            vec![
                "前段正常内容用于验证完整预缓冲。",
                "末段延迟的音声应该在准备完成后连续播放。",
            ]
        } else if long {
            vec![
                "第一句是流式输出中的正常正文，不应该包含内部思考。",
                "第二句用于检查语音片段乱序返回时，仍然按照原文顺序播放。",
                "第三句继续检查中文、emoji 🙂与长文字分页；整条回复结束后才能让出任务。",
            ]
        } else {
            vec![
                "第一句是流式输出中的正常正文。",
                "第二句检查语音片段的连续播放。",
            ]
        };
        let mut data = String::new();
        data.push_str(&format!("data: {}\n\n",json!({"choices":[{"delta":{"reasoning_content":"hidden-fixture-reasoning","tool_calls":[{"function":{"arguments":"hidden-tool-json"}}]}}]})));
        for text in chunks {
            for part in text.chars().collect::<Vec<_>>().chunks(5) {
                data.push_str(&format!(
                    "data: {}\n\n",
                    json!({"choices":[{"delta":{"content":part.iter().collect::<String>()}}]})
                ));
            }
        }
        if !input.contains("[cut]") {
            data.push_str("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n");
        }
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
        )?;
        for (index, line) in data.split_inclusive("\n\n").enumerate() {
            if input.contains("[pause]") && index == 4 {
                thread::sleep(Duration::from_millis(650));
            }
            stream.write_all(line.as_bytes())?;
            stream.flush()?;
            thread::sleep(Duration::from_millis(35));
        }
        // Deliver the final SSE bytes before EOF, including deliberate missing-DONE
        // cases. A reset on Windows may otherwise discard already-written data.
        stream.shutdown(std::net::Shutdown::Write)?;
        let _ = stream.read(&mut [0u8; 1]);
        Ok(())
    } else if header.lines().next().unwrap_or("").contains("tts") {
        let text = body["text"].as_str().unwrap_or("");
        thread::sleep(Duration::from_millis(
            if text.contains("第一句") || text.contains("末段延迟") {
                700
            } else {
                50
            },
        ));
        let samples = 8000usize;
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend(((36 + samples * 2) as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(8000u32.to_le_bytes());
        wav.extend(16000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(((samples * 2) as u32).to_le_bytes());
        for i in 0..samples {
            let sample = ((i as f64 * 440.0 * std::f64::consts::TAU / 8000.0).sin() * 600.0) as i16;
            wav.extend(sample.to_le_bytes());
        }
        response(&mut stream, "200 OK", "audio/wav", &wav)
    } else {
        response(&mut stream, "404 Not Found", "application/json", b"{}")
    }
}
