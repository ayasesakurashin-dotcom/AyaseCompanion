use crate::schema;
use image::ImageReader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
    path::Path,
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

pub const MAX_FILE: u64 = 128 * 1024 * 1024;
pub const MAX_TOTAL: u64 = 512 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageReport {
    pub kind: String,
    pub id: String,
    pub version: String,
    pub display_name: String,
    pub warnings: Vec<String>,
    pub hash: String,
    pub file_count: usize,
    pub expanded_bytes: u64,
}
#[derive(Debug, Clone)]
pub struct Package {
    pub files: BTreeMap<String, Vec<u8>>,
    pub manifest: Value,
    pub model: Option<Value>,
    pub report: PackageReport,
}

pub fn digest(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
pub fn safe_path(name: &str) -> Result<String, String> {
    if name.is_empty()
        || name.len() > 240
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains(':')
        || name
            .chars()
            .any(|c| c.is_control() || "<>\"|?*".contains(c))
    {
        return Err(format!("不安全的包内路径：{name}"));
    }
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' ']) {
            return Err(format!("不安全的路径段：{name}"));
        }
        let stem = part.split('.').next().unwrap().to_uppercase();
        if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"]
                    .contains(&&stem[3..]))
        {
            return Err(format!("Windows 保留文件名：{name}"));
        }
    }
    Ok(name.to_lowercase())
}
pub fn read_zip(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    if bytes.len() as u64 > MAX_TOTAL {
        return Err("压缩包超过 512 MiB".into());
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("ZIP 损坏：{e}"))?;
    if zip.len() > 2048 {
        return Err("ZIP 条目超过 2048".into());
    }
    let mut seen = BTreeSet::new();
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().to_string();
        let norm = safe_path(name.trim_end_matches('/'))?;
        if !seen.insert(norm) {
            return Err(format!("重复规范化路径：{name}"));
        }
        if f.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err("不允许符号链接".into());
        }
        if f.is_dir() {
            continue;
        }
        total = total.checked_add(f.size()).ok_or("包体积溢出")?;
        if total > MAX_TOTAL || f.size() > MAX_FILE {
            return Err("解压大小超过限制".into());
        }
        if f.size() > 1024 * 1024 && f.size() / f.compressed_size().max(1) > 500 {
            return Err("异常解压比".into());
        }
        let mut data = Vec::with_capacity(f.size() as usize);
        (&mut f)
            .take(MAX_FILE + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if data.len() as u64 > MAX_FILE || data.len() as u64 != f.size() {
            return Err("ZIP 长度不一致".into());
        }
        files.insert(name, data);
    }
    // A file cannot also be the parent directory of another entry.
    for name in files.keys() {
        let mut parent = name.as_str();
        while let Some((p, _)) = parent.rsplit_once('/') {
            if files.keys().any(|x| x.eq_ignore_ascii_case(p)) {
                return Err("文件与目录路径冲突".into());
            }
            parent = p;
        }
    }
    Ok(files)
}
pub fn json_file(files: &BTreeMap<String, Vec<u8>>, path: &str) -> Result<Value, String> {
    safe_path(path)?;
    let b = files.get(path).ok_or_else(|| format!("缺少 {path}"))?;
    if b.len() > 2 * 1024 * 1024 {
        return Err(format!("JSON 过大：{path}"));
    }
    serde_json::from_slice(b).map_err(|e| format!("{path}: {e}"))
}
pub fn validate(bytes: &[u8]) -> Result<Package, String> {
    let files = read_zip(bytes)?;
    let is_character = files.contains_key("manifest.json");
    if is_character && files.contains_key("plugin.json") {
        return Err("角色和插件入口不能混用".into());
    }
    let (entry, kind) = if is_character {
        ("manifest.json", "character")
    } else {
        ("plugin.json", "plugin")
    };
    let manifest = json_file(&files, entry)?;
    schema::validate(&schema::contract(kind), &manifest)?;
    no_private_fields(&manifest)?;
    if let Some(integrity) = manifest["integrity"].as_object() {
        for (path, hash) in integrity {
            safe_path(path)?;
            let b = files
                .get(path)
                .ok_or_else(|| format!("完整性清单文件缺失：{path}"))?;
            if hash.as_str() != Some(&digest(b)) {
                return Err(format!("完整性散列不匹配：{path}"));
            }
        }
    }
    let mut warnings = vec![];
    let mut model = None;
    if is_character {
        let assets = manifest["assets"].as_object().unwrap();
        let mut dims = BTreeMap::new();
        let mut total_pixels = 0u64;
        for (id, asset) in assets {
            let path = asset["path"].as_str().unwrap();
            safe_path(path)?;
            if let Some(b) = files.get(path) {
                if let Some(hash) = asset["sha256"].as_str() {
                    if digest(b) != hash {
                        return Err(format!("资产散列不符：{id}"));
                    }
                }
                if asset["kind"] == "image" {
                    match image_dimensions(b) {
                        Ok((w, h)) => {
                            total_pixels += w as u64 * h as u64;
                            if total_pixels > 128 * 1024 * 1024 {
                                return Err("图像总解码预算超过 128 M 像素".into());
                            }
                            dims.insert(id.clone(), (w, h));
                        }
                        Err(e) => warnings.push(format!("可选图像 {id} 无法解码：{e}")),
                    }
                }
            } else {
                warnings.push(format!("可选资产缺失：{id}"));
            }
        }
        let avatar = if manifest["avatar"]["renderer"] == "sprite.v1" {
            &manifest["avatar"]
        } else {
            let fallback = &manifest["avatar"]["fallback"];
            if fallback["renderer"] != "sprite.v1" {
                return Err("不支持的渲染器且无 sprite.v1 后备".into());
            }
            warnings.push("使用声明的后备模型".into());
            fallback
        };
        let desc = avatar["descriptor"].as_str().ok_or("缺少模型描述路径")?;
        let mut m = json_file(&files, desc)?;
        schema::validate(&schema::contract("sprite"), &m)?;
        let idle = m["baseModes"]["idle"]["loop"]
            .as_str()
            .ok_or("缺少 idle.loop")?
            .to_owned();
        if !m["clips"][&idle].is_object() {
            return Err("idle 片段不存在".into());
        }
        let mut missing = vec![];
        for (id, clip) in m["clips"].as_object().unwrap() {
            let asset = clip["asset"].as_str().unwrap();
            if let Some(&(w, h)) = dims.get(asset) {
                validate_layout(clip, w, h).map_err(|e| format!("clips/{id}: {e}"))?;
            } else if id == &idle {
                return Err("必需 idle 图像缺失或损坏".into());
            } else {
                missing.push(id.clone());
                warnings.push(format!("动作 {id} 缺少有效图像，将保持基础姿态"))
            }
        }
        // Runtime projection only; original descriptor bytes are retained for round-trips.
        for id in missing {
            m["clips"].as_object_mut().unwrap().remove(&id);
        }
        let unavailable_modes: Vec<_> = m["baseModes"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, v)| !m["clips"][v["loop"].as_str().unwrap()].is_object())
            .map(|(k, _)| k.clone())
            .collect();
        for name in unavailable_modes {
            if name == "idle" {
                return Err("idle 模式无有效片段".into());
            }
            m["baseModes"].as_object_mut().unwrap().remove(&name);
            warnings.push(format!("基础姿态 {name} 缺少可用循环，降级到 idle"));
        }
        model = Some(m);
        let mut ids = BTreeSet::new();
        if let Some(presets) = manifest["pluginPresets"].as_array() {
            for p in presets {
                if !ids.insert(p["presetId"].as_str().unwrap()) {
                    return Err("presetId 重复".into());
                }
            }
        }
    } else {
        crate::plugin::validate_package(&manifest, &files)?;
    }
    let report = PackageReport {
        kind: kind.into(),
        id: manifest[if is_character {
            "packageId"
        } else {
            "pluginId"
        }]
        .as_str()
        .unwrap()
        .into(),
        version: manifest["version"].as_str().unwrap().into(),
        display_name: manifest["displayName"].as_str().unwrap().into(),
        warnings,
        hash: digest(bytes),
        file_count: files.len(),
        expanded_bytes: files.values().map(|v| v.len() as u64).sum(),
    };
    Ok(Package {
        files,
        manifest,
        model,
        report,
    })
}
pub fn image_dimensions(b: &[u8]) -> Result<(u32, u32), String> {
    let mut reader = ImageReader::new(Cursor::new(b))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::WebP)
    ) {
        return Err("仅支持 PNG/WebP".into());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| e.to_string())?;
    Ok((img.width(), img.height()))
}
fn number(v: &Value, key: &str) -> Result<u64, String> {
    v[key]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("{key} 必须是正整数"))
}
fn validate_layout(c: &Value, w: u32, h: u32) -> Result<(), String> {
    let l = &c["layout"];
    let count = match l["type"].as_str().unwrap() {
        "single" => 1,
        "grid" => {
            let columns = number(l, "columns")?;
            let rows = number(l, "rows")?;
            let count = number(l, "frameCount")?;
            if count > columns * rows
                || columns * number(l, "cellWidth")? > w as u64
                || rows * number(l, "cellHeight")? > h as u64
            {
                return Err("图集网格超出图片边界".into());
            }
            count
        }
        "rects" => {
            let frames = l["frames"].as_array().ok_or("需要 frames")?;
            for f in frames {
                if f["x"].as_u64().unwrap().saturating_add(number(f, "width")?) > w as u64
                    || f["y"]
                        .as_u64()
                        .unwrap()
                        .saturating_add(number(f, "height")?)
                        > h as u64
                {
                    return Err("帧矩形越界".into());
                }
            }
            frames.len() as u64
        }
        _ => return Err("未知布局".into()),
    };
    if let Some(a) = c["frameDurationsMs"].as_array() {
        if a.len() as u64 != count {
            return Err("逐帧时长与帧数不一致".into());
        }
    }
    Ok(())
}
fn no_private_fields(v: &Value) -> Result<(), String> {
    match v {
        Value::Object(o) => {
            for (k, v) in o {
                if [
                    "instanceid",
                    "memorybinding",
                    "bankid",
                    "apikey",
                    "credentials",
                    "historyepoch",
                    "localdatadir",
                ]
                .contains(&k.to_lowercase().as_str())
                {
                    return Err(format!("可分发包不允许本机私有字段：{k}"));
                }
                no_private_fields(v)?
            }
        }
        Value::Array(a) => {
            for v in a {
                no_private_fields(v)?
            }
        }
        _ => {}
    }
    Ok(())
}
pub fn pack(
    mut files: BTreeMap<String, Vec<u8>>,
    mut manifest: Value,
    entry: &str,
) -> Result<Vec<u8>, String> {
    files.remove(entry);
    let integrity: serde_json::Map<String, Value> = files
        .iter()
        .map(|(p, b)| (p.clone(), json!(digest(b))))
        .collect();
    manifest["integrity"] = Value::Object(integrity);
    no_private_fields(&manifest)?;
    files.insert(
        entry.into(),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, data) in files {
        safe_path(&path)?;
        writer
            .start_file(
                path,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(|e| e.to_string())?;
        writer.write_all(&data).map_err(|e| e.to_string())?;
    }
    Ok(writer.finish().map_err(|e| e.to_string())?.into_inner())
}
impl Package {
    pub fn write_to(&self, root: &Path) -> Result<(), String> {
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        for (path, data) in &self.files {
            safe_path(path)?;
            let target = root.join(path);
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?
            }
            std::fs::write(target, data).map_err(|e| e.to_string())?
        }
        Ok(())
    }
    pub fn asset_text(&self, id: &str) -> Option<String> {
        let path = self.manifest["assets"][id]["path"].as_str()?;
        let b = self.files.get(path)?;
        if b.len() > 64 * 1024 {
            return None;
        }
        String::from_utf8(b.clone()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_security() {
        for p in [
            "../x",
            "C:/x",
            "/x",
            "a\\b",
            "CON.txt",
            "com1.png",
            "a./x",
            "a /x",
            "a//b",
            "a:b",
            "a/../../x",
        ] {
            assert!(safe_path(p).is_err(), "{p}")
        }
        assert!(safe_path("assets/艾玛.webp").is_ok());
    }
    #[test]
    fn zip_case_duplicate_and_symlink() {
        let mut z = ZipWriter::new(Cursor::new(Vec::new()));
        for p in ["A.txt", "a.txt"] {
            z.start_file(p, SimpleFileOptions::default()).unwrap();
            z.write_all(b"x").unwrap();
        }
        assert!(read_zip(&z.finish().unwrap().into_inner()).is_err());
    }
    #[test]
    fn corrupt_image_is_error() {
        assert!(image_dimensions(b"not an image").is_err());
    }
    #[test]
    fn layout_bounds() {
        assert!(validate_layout(&json!({"layout":{"type":"grid","columns":2,"rows":1,"frameCount":2,"cellWidth":20,"cellHeight":20}}),20,20).is_err());
    }
}
