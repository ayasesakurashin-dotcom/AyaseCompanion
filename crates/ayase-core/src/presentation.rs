use crate::{package::Package, plugin};
use serde_json::{json, Value};
use std::path::Path;
pub fn resolve(
    p: &Package,
    overrides: &Value,
    muted: bool,
    content_dir: &Path,
    v: &Value,
    owner: &str,
) -> Result<(Value, Vec<String>), String> {
    let asset_enabled = |id: &str| {
        overrides["assetEnabled"][id]
            .as_bool()
            .or_else(|| p.manifest["assets"][id]["enabled"].as_bool())
            .unwrap_or(true)
    };
    let mut plan = json!({"repeat":1});
    if let Some(read_ms) = v["readMs"].as_u64() {
        plan["readMs"] = json!(read_ms);
    }
    let mut channels = vec![];
    let mut texts = plugin::material_values(&v["text"], false)?;
    if texts.is_empty() {
        texts = plugin::material_values(&v["textAsset"], true)?
            .iter()
            .filter_map(|id| p.asset_text(id))
            .filter(|s| !s.trim().is_empty())
            .collect();
    }
    if texts.iter().map(|s| s.chars().count()).sum::<usize>() > 12000 {
        return Err("同一条回复的文字素材合计不能超过12000字".into());
    }
    let texts: Vec<_> = texts.into_iter().map(|s| s.replace("\r\n", "\n")).collect();
    if !texts.is_empty() {
        plan["text"] = json!(texts.join("\n"));
        plan["textSegments"] = json!(texts);
        channels.push("text".into());
    }
    let mut audio = vec![];
    for id in plugin::material_values(&v["audioAsset"], true)? {
        if let Some(path) = p.manifest["assets"][&id]["path"]
            .as_str()
            .filter(|_| asset_enabled(&id))
        {
            if p.manifest["assets"][&id]["kind"] == "audio" && p.files.contains_key(path) && !muted
            {
                let path = content_dir.join(path).to_string_lossy().into_owned();
                audio.push(
                    json!({"path":path,"durationMs":p.manifest["assets"][&id]["durationMs"]}),
                );
            }
        }
    }
    if !audio.is_empty() {
        plan["audioSegments"] = json!(audio);
        plan["audioDone"] = json!(true);
        channels.push("audio".into());
    }
    let mut clips = vec![];
    for cue in plugin::material_values(&v["cue"], true)? {
        let model = p.model.as_ref().unwrap();
        let mapping = overrides["semanticCues"]
            .get(&cue)
            .or_else(|| model["semanticCues"].get(&cue));
        if let Some(mapping) = mapping {
            if let Some(clip) = mapping["clip"].as_str() {
                if model["clips"][clip].is_object() {
                    clips.extend(std::iter::repeat_n(
                        clip.to_owned(),
                        mapping["repeat"].as_u64().unwrap_or(1).clamp(1, 8) as usize,
                    ));
                }
            }
        }
        // Explicit UI clip preview is represented as a host-only cue.
        if owner == "host.user" && cue.starts_with("clip:") {
            let clip = &cue[5..];
            if model["clips"][clip].is_object() {
                clips.push(clip.to_owned());
            }
        }
    }
    let model = p.model.as_ref().unwrap();
    for id in plugin::material_values(&v["animationAsset"], true)? {
        let direct = id.strip_prefix("clip:").unwrap_or(&id);
        if model["clips"][direct].is_object() {
            clips.push(direct.to_owned());
            continue;
        }
        let matches: Vec<_> = model["clips"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, clip)| clip["asset"] == id)
            .map(|(name, _)| name.clone())
            .collect();
        if matches.len() > 1 {
            return Err(format!("动画素材{id}对应多个片段，请选择具体片段编号"));
        }
        clips.extend(matches);
    }
    if !clips.is_empty() {
        plan["clips"] = json!(clips);
        channels.push("animation".into());
    }
    Ok((plan, channels))
}
