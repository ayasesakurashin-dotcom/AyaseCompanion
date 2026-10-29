//! Private, per-instance local voice samples. Importing never uploads.
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn directory(root: &Path, owner: &str) -> Result<PathBuf, String> {
    let id = uuid::Uuid::parse_str(owner).map_err(|_| "伙伴身份无效")?;
    Ok(root.join("voice-samples").join(id.simple().to_string()))
}
pub fn list(root: &Path, owner: &str) -> Result<Vec<Value>, String> {
    let dir = directory(root, owner)?;
    let mut out = vec![];
    if !dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(&dir)
        .map_err(|_| "本地音声目录不可读")?
        .flatten()
    {
        if entry.path().extension().is_some_and(|s| s == "json") {
            let raw = fs::read(entry.path()).map_err(|_| "音声记录不可读")?;
            if raw.len() > 8192 {
                return Err("音声记录过大".into());
            }
            let v: Value = serde_json::from_slice(&raw).map_err(|_| "音声记录格式无效")?;
            out.push(v);
        }
    }
    out.sort_by_key(|v| v["id"].as_str().unwrap_or("").to_owned());
    Ok(out)
}
pub fn load(root: &Path, owner: &str, id: &str) -> Result<(Value, Vec<u8>), String> {
    let id = id.strip_prefix("local-").ok_or("本地素材ID无效")?;
    let id = uuid::Uuid::parse_str(id)
        .map_err(|_| "本地素材ID无效")?
        .simple()
        .to_string();
    let dir = directory(root, owner)?;
    let raw = fs::read(dir.join(format!("{id}.json"))).map_err(|_| "本地音声记录不存在")?;
    if raw.len() > 8192 {
        return Err("音声记录无效".into());
    }
    let meta: Value = serde_json::from_slice(&raw).map_err(|_| "音声记录格式错误")?;
    let extension = meta["extension"]
        .as_str()
        .filter(|e| ["wav", "mp3"].contains(e))
        .ok_or("音声格式无效")?;
    let path = dir.join(format!("{id}.{extension}"));
    if fs::metadata(&path).map_err(|_| "本地音声不存在")?.len() > 8 * 1024 * 1024 {
        return Err("本地音声过大".into());
    }
    Ok((meta, fs::read(path).map_err(|_| "本地音声读取失败")?))
}
pub fn import(root: &Path, owner: &str, path: &Path) -> Result<Value, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["wav", "mp3"].contains(&ext.as_str()) {
        return Err("本地暂存只接受 WAV/MP3".into());
    }
    let len = fs::metadata(path).map_err(|_| "所选音声无法读取")?.len();
    let list = list(root, owner)?;
    let total: u64 = list.iter().filter_map(|v| v["size"].as_u64()).sum();
    if list.len() >= 8 || len == 0 || len > 8 * 1024 * 1024 || total + len > 20 * 1024 * 1024 {
        return Err("最多暂存8段；单段8 MiB、合计20 MiB".into());
    }
    let bytes = fs::read(path).map_err(|_| "所选音声读取失败")?;
    if ext == "wav" && !(bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"))
        || ext == "mp3" && !(bytes.starts_with(b"ID3") || bytes.first() == Some(&255))
    {
        return Err("文件内容与音声类型不匹配".into());
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let dir = directory(root, owner)?;
    fs::create_dir_all(&dir).map_err(|_| "暂存目录不可写")?;
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("录音");
    let meta = json!({"id":format!("local-{id}"),"name":crate::memory::bounded(name,100),"extension":ext,"size":bytes.len(),"transcript":"","local":true});
    let audio = dir.join(format!("{id}.{ext}"));
    fs::write(&audio, &bytes).map_err(|_| "音声暂存失败")?;
    if fs::write(dir.join(format!("{id}.json")), meta.to_string()).is_err() {
        let _ = fs::remove_file(audio);
        return Err("音声记录保存失败".into());
    }
    Ok(meta)
}
pub fn remove(root: &Path, owner: &str, id: &str) -> Result<(), String> {
    let (meta, _) = load(root, owner, id)?;
    let parsed = uuid::Uuid::parse_str(id.strip_prefix("local-").unwrap())
        .unwrap()
        .simple()
        .to_string();
    let dir = directory(root, owner)?;
    let path = dir.join(format!("{parsed}.{}", meta["extension"].as_str().unwrap()));
    fs::remove_file(path).map_err(|_| "音声移除失败")?;
    fs::remove_file(dir.join(format!("{parsed}.json"))).map_err(|_| "音声记录移除失败")?;
    Ok(())
}
