use crate::{package, schema};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Data-only appearance. No CSS, HTML, network URLs or scripts are accepted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Theme {
    pub format: String,
    pub format_version: String,
    pub display_name: String,
    pub foreground: String,
    pub background: String,
    pub border_color: String,
    pub fill_opacity: u32,
    pub border_radius: u32,
    pub padding_top: u32,
    pub padding_right: u32,
    pub padding_bottom: u32,
    pub padding_left: u32,
    pub tail_size: u32,
    pub tail_x: u32,
    pub font_size: u32,
    pub font_family: String,
    pub animation: String,
    pub characters_per_second: u32,
    pub read_ms: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub input_opacity: u32,
    pub background_asset: Option<String>,
    pub slice_top: u32,
    pub slice_right: u32,
    pub slice_bottom: u32,
    pub slice_left: u32,
    pub assets: BTreeMap<String, Value>,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            format: "ayase.theme".into(),
            format_version: "1.0".into(),
            display_name: "内置樱色".into(),
            foreground: "#493c4d".into(),
            background: "#fffafe".into(),
            border_color: "#dfc8da".into(),
            fill_opacity: 98,
            border_radius: 16,
            padding_top: 12,
            padding_right: 16,
            padding_bottom: 12,
            padding_left: 16,
            tail_size: 12,
            tail_x: 50,
            font_size: 15,
            font_family: "Segoe UI".into(),
            animation: "typewriter".into(),
            characters_per_second: 30,
            read_ms: 2800,
            offset_x: 0,
            offset_y: 0,
            max_width: None,
            max_height: None,
            input_opacity: 100,
            background_asset: None,
            slice_top: 12,
            slice_right: 12,
            slice_bottom: 12,
            slice_left: 12,
            assets: BTreeMap::new(),
        }
    }
}
pub fn parse(value: &Value, files: &BTreeMap<String, Vec<u8>>) -> Result<Theme, String> {
    schema::validate(&schema::contract("theme"), value)?;
    let theme: Theme = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    if theme.max_height.is_some_and(|h| {
        h < theme.padding_top
            + theme.padding_bottom
            + 20
            + (theme.font_size as f64 * 1.65).ceil() as u32
    }) {
        return Err("手动最大高度不足以容纳一行文字、内边距和页码".into());
    }
    if theme
        .max_width
        .is_some_and(|w| w < theme.padding_left + theme.padding_right + 2 + theme.font_size * 2)
    {
        return Err("手动最大宽度不足以容纳文字与内边距".into());
    }
    let mut pixels = 0u64;
    for asset in theme.assets.values() {
        let path = asset["path"].as_str().ok_or("主题素材缺少路径")?;
        package::safe_path(path)?;
        if asset["kind"] != "image" {
            return Err("主题只能附带PNG/WebP图像".into());
        }
        let bytes = files.get(path).ok_or("主题素材缺失")?;
        let (w, h) = package::image_dimensions(bytes)?;
        pixels += w as u64 * h as u64;
        if pixels > 128 * 1024 * 1024 {
            return Err("主题图像总预算超限".into());
        }
    }
    if let Some(id) = &theme.background_asset {
        let asset = theme.assets.get(id).ok_or("主题背景素材未登记")?;
        if asset["kind"] != "image" {
            return Err("主题背景必须是PNG/WebP图像".into());
        }
        let path = asset["path"].as_str().ok_or("主题素材缺少路径")?;
        package::safe_path(path)?;
        let bytes = files.get(path).ok_or("主题背景文件缺失")?;
        let (w, h) = package::image_dimensions(bytes)?;
        if theme.slice_left + theme.slice_right >= w || theme.slice_top + theme.slice_bottom >= h {
            return Err("九宫格边距必须小于背景图像尺寸".into());
        }
    }
    Ok(theme)
}
pub fn defaults() -> Value {
    json!(Theme::default())
}
