//! Authoring drafts are separate from runtime instances. Exports always pass the
//! same package/plugin/schema validators as ordinary imports.
use crate::{
    config,
    package::{self, Package},
    plugin::{self, Plugin},
    theme,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub format: String,
    pub format_version: String,
    pub kind: String,
    pub revision: u64,
    pub manifest: Value,
    pub model: Value,
    pub config_schema: Value,
    pub rules: Value,
    pub theme: Value,
}
#[derive(Clone)]
pub struct Project {
    pub document: Document,
    pub files: BTreeMap<String, Vec<u8>>,
}
impl Project {
    pub fn character() -> Self {
        Self {
            document: Document {
                format: "ayase.project".into(),
                format_version: "1.0".into(),
                kind: "character".into(),
                revision: 0,
                manifest: json!({"format":"ayase.character","formatVersion":"1.0","packageId":"local.my-companion","version":"0.1.0","displayName":"我的伙伴","identity":{"characterName":"伙伴","userAddress":"你","persona":"自然、友善地与用户交流。"},"avatar":{"renderer":"sprite.v1","descriptor":"avatar/sprite.json"},"assets":{},"pluginPresets":[]}),
                model: json!({"schemaVersion":"1.0","logicalSize":{"width":360,"height":280},"anchors":{"feet":{"x":0.5,"y":1},"bubble":{"x":0.5,"y":0}},"clips":{},"baseModes":{"idle":{"loop":"idle"}},"semanticCues":{}}),
                config_schema: Value::Null,
                rules: Value::Null,
                theme: Value::Null,
            },
            files: BTreeMap::new(),
        }
    }
    pub fn rule_plugin() -> Self {
        let manifest: Value = serde_json::from_str(include_str!(
            "../../../plugins/examples/nightly-rest/plugin.json"
        ))
        .unwrap();
        let config_schema = serde_json::from_str(include_str!(
            "../../../plugins/examples/nightly-rest/schemas/config.schema.json"
        ))
        .unwrap();
        let rules = serde_json::from_str(include_str!(
            "../../../plugins/examples/nightly-rest/rules/main.json"
        ))
        .unwrap();
        Self {
            document: Document {
                format: "ayase.project".into(),
                format_version: "1.0".into(),
                kind: "plugin".into(),
                revision: 0,
                manifest,
                model: Value::Null,
                config_schema,
                rules,
                theme: Value::Null,
            },
            files: BTreeMap::new(),
        }
    }
    pub fn theme() -> Self {
        let mut p = Self::character();
        p.document.kind = "theme".into();
        p.document.manifest = theme::defaults();
        p.document.model = Value::Null;
        p
    }
    pub fn from_package(p: Package) -> Result<Self, String> {
        let mut d = Self::character();
        d.document.kind = p.report.kind.clone();
        d.document.manifest = p.manifest.clone();
        d.files = p.files;
        d.document.model = if p.report.kind == "character" {
            package::json_file(
                &d.files,
                (if p.manifest["avatar"]["renderer"] == "sprite.v1" {
                    &p.manifest["avatar"]
                } else {
                    &p.manifest["avatar"]["fallback"]
                })["descriptor"]
                    .as_str()
                    .ok_or("缺少模型描述")?,
            )?
        } else {
            Value::Null
        };
        if p.report.kind == "plugin" {
            d.document.config_schema =
                package::json_file(&d.files, p.manifest["configSchema"].as_str().unwrap())?;
            d.document.rules =
                package::json_file(&d.files, p.manifest["runtime"]["entry"].as_str().unwrap())?;
        }
        if let Some(path) = p.manifest["recommendedTheme"].as_str() {
            if let Ok(v) = package::json_file(&d.files, path) {
                if theme::parse(&v, &d.files).is_ok() {
                    d.document.theme = v;
                }
            }
        }
        for entry in ["manifest.json", "plugin.json", "theme.json"] {
            d.files.remove(entry);
        }
        Ok(d)
    }
    pub fn save_document(&mut self, expected: u64, mut next: Document) -> Result<(), String> {
        if self.document.revision != expected {
            return Err("项目已被更新，请重新载入后编辑".into());
        }
        if next.format != "ayase.project"
            || next.format_version != "1.0"
            || next.kind != self.document.kind
        {
            return Err("项目类型或格式不匹配".into());
        }
        // Drafts may be incomplete, but private runtime fields never become author data.
        package::no_private_fields(
            &json!({"manifest":next.manifest,"model":next.model,"schema":next.config_schema,"rules":next.rules,"theme":next.theme}),
        )?;
        next.revision = expected + 1;
        self.document = next;
        Ok(())
    }
    pub fn export(&self) -> Result<Vec<u8>, String> {
        let mut files = self.files.clone();
        let mut manifest = self.document.manifest.clone();
        manifest
            .as_object_mut()
            .ok_or("清单必须是对象")?
            .remove("integrity");
        if let Some(assets) = manifest["assets"].as_object_mut() {
            for a in assets.values_mut() {
                if let Some(o) = a.as_object_mut() {
                    o.remove("sha256");
                }
            }
        }
        let entry = match self.document.kind.as_str() {
            "character" => {
                let path = (if manifest["avatar"]["renderer"] == "sprite.v1" {
                    &manifest["avatar"]
                } else {
                    &manifest["avatar"]["fallback"]
                })["descriptor"]
                    .as_str()
                    .ok_or("缺少模型描述路径")?
                    .to_owned();
                package::safe_path(&path)?;
                files.insert(
                    path,
                    serde_json::to_vec_pretty(&self.document.model).unwrap(),
                );
                if !self.document.theme.is_null() {
                    theme::parse(&self.document.theme, &files)?;
                    let path = "theme/recommended.json";
                    manifest["recommendedTheme"] = json!(path);
                    files.insert(
                        path.into(),
                        serde_json::to_vec_pretty(&self.document.theme).unwrap(),
                    );
                }
                "manifest.json"
            }
            "plugin" => {
                for (key, value) in [("configSchema", &self.document.config_schema)] {
                    let path = manifest[key].as_str().ok_or("插件缺少schema路径")?;
                    package::safe_path(path)?;
                    files.insert(path.into(), serde_json::to_vec_pretty(value).unwrap());
                }
                let path = manifest["runtime"]["entry"]
                    .as_str()
                    .ok_or("插件缺少规则入口")?;
                package::safe_path(path)?;
                files.insert(
                    path.into(),
                    serde_json::to_vec_pretty(&self.document.rules).unwrap(),
                );
                "plugin.json"
            }
            "theme" => {
                theme::parse(&manifest, &files)?;
                "theme.json"
            }
            _ => return Err("未知项目类型".into()),
        };
        let bytes = package::pack(files, manifest, entry)?;
        package::validate(&bytes)?;
        Ok(bytes)
    }
    pub fn validated(&self) -> Result<Package, String> {
        package::validate(&self.export()?)
    }
    pub fn asset(
        &mut self,
        id: &str,
        kind: &str,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        if id.is_empty() || id.len() > 128 || id.contains([',', '\n', '\r']) {
            return Err("素材编号为空、过长或含分隔符".into());
        }
        package::safe_path(path)?;
        if bytes.len() as u64 > package::MAX_FILE {
            return Err("素材超过单文件限制".into());
        }
        if kind == "image" {
            package::image_dimensions(&bytes)?;
        } else if kind == "text" {
            if bytes.len() > 65536 || std::str::from_utf8(&bytes).is_err() {
                return Err("文字素材须为64KiB内UTF-8".into());
            }
        } else if !["audio", "data"].contains(&kind) {
            return Err("未知素材类型".into());
        }
        if self
            .files
            .iter()
            .filter(|(p, _)| p.as_str() != path)
            .map(|(_, b)| b.len() as u64)
            .sum::<u64>()
            + bytes.len() as u64
            > package::MAX_TOTAL
        {
            return Err("项目素材超过总预算".into());
        }
        if self.document.manifest["assets"].is_null() {
            self.document.manifest["assets"] = json!({});
        }
        self.files.insert(path.into(), bytes);
        self.document.manifest["assets"][id] = json!({"kind":kind,"path":path});
        self.document.revision += 1;
        Ok(())
    }
    pub fn references(&self, id: &str) -> Vec<String> {
        fn walk(v: &Value, id: &str, path: &str, out: &mut Vec<String>) {
            match v {
                Value::String(s)
                    if s == id || s.split([',', '，', '\n', '\r']).any(|p| p.trim() == id) =>
                {
                    out.push(path.into())
                }
                Value::Object(o) => {
                    for (k, v) in o {
                        if !(path == "/manifest/assets" && k == id) {
                            walk(
                                v,
                                id,
                                &format!("{path}/{}", k.replace('~', "~0").replace('/', "~1")),
                                out,
                            );
                        }
                    }
                }
                Value::Array(a) => {
                    for (i, v) in a.iter().enumerate() {
                        walk(v, id, &format!("{path}/{i}"), out);
                    }
                }
                _ => {}
            }
        }
        let mut out = vec![];
        walk(
            &json!({"manifest":self.document.manifest,"model":self.document.model,"theme":self.document.theme}),
            id,
            "",
            &mut out,
        );
        out
    }
    pub fn rename_asset(&mut self, from: &str, to: &str) -> Result<(), String> {
        self.rename_asset_known(from, to, &plugin::builtin().keys().cloned().collect())
    }
    pub fn rename_asset_known(
        &mut self,
        from: &str,
        to: &str,
        known: &std::collections::BTreeSet<String>,
    ) -> Result<(), String> {
        if to.is_empty()
            || to.len() > 128
            || to.contains([',', '\n', '\r'])
            || self.document.manifest["assets"].get(to).is_some()
        {
            return Err("新编号无效或已存在".into());
        }
        let unknown: Vec<_> = self.document.manifest["pluginPresets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| !known.contains(p["pluginId"].as_str().unwrap_or("")))
            .filter_map(|p| p["presetId"].as_str())
            .collect();
        if !unknown.is_empty()
            && self
                .references(from)
                .iter()
                .any(|p| p.starts_with("/manifest/pluginPresets/"))
        {
            return Err("未知插件可能引用该编号，请先安装/确认插件，或保留旧编号".into());
        }
        fn replace(v: &mut Value, from: &str, to: &str) {
            match v {
                Value::String(s) => {
                    if s == from {
                        *s = to.into();
                    } else {
                        *s = s
                            .split_inclusive([',', '，', '\n', '\r'])
                            .map(|p| {
                                if p.trim_matches([',', '，', '\n', '\r', ' ']) == from {
                                    p.replacen(from, to, 1)
                                } else {
                                    p.to_owned()
                                }
                            })
                            .collect();
                    }
                }
                Value::Array(a) => {
                    for v in a {
                        replace(v, from, to);
                    }
                }
                Value::Object(o) => {
                    for v in o.values_mut() {
                        replace(v, from, to);
                    }
                }
                _ => {}
            }
        }
        let asset = self.document.manifest["assets"]
            .as_object_mut()
            .ok_or("无素材库")?
            .remove(from)
            .ok_or("素材不存在")?;
        replace(&mut self.document.manifest, from, to);
        replace(&mut self.document.model, from, to);
        replace(&mut self.document.theme, from, to);
        if let Some(assets) = self
            .document
            .theme
            .get_mut("assets")
            .and_then(Value::as_object_mut)
        {
            if let Some(asset) = assets.remove(from) {
                assets.insert(to.into(), asset);
            }
        }
        self.document.manifest["assets"][to] = asset;
        self.document.revision += 1;
        Ok(())
    }
    pub fn delete_asset(&mut self, id: &str) -> Result<(), String> {
        let refs = self.references(id);
        if !refs.is_empty() {
            return Err(format!("素材仍被引用：{}", refs.join("、")));
        }
        let item = self.document.manifest["assets"]
            .as_object_mut()
            .ok_or("无素材库")?
            .remove(id)
            .ok_or("素材不存在")?;
        if let Some(assets) = self
            .document
            .theme
            .get_mut("assets")
            .and_then(Value::as_object_mut)
        {
            assets.remove(id);
        }
        if let Some(path) = item["path"].as_str() {
            if !self.document.manifest["assets"]
                .as_object()
                .unwrap()
                .values()
                .any(|v| v["path"] == path)
            {
                self.files.remove(path);
            }
        }
        self.document.revision += 1;
        Ok(())
    }
}

pub fn effective_presets(
    manifest: &Value,
    plugins: &BTreeMap<String, Plugin>,
) -> Result<Value, String> {
    let mut out = vec![];
    for preset in manifest["pluginPresets"].as_array().into_iter().flatten() {
        if let Some(p) = plugins.get(preset["pluginId"].as_str().unwrap_or("")) {
            let c = p.configure(preset, &json!({}), 0);
            if !["已启用", "已关闭"].contains(&c.status.as_str()) {
                return Err(format!("{}：{}", c.owner, c.status));
            }
            let mut v = preset.clone();
            v["config"] = c.config;
            v["configMode"] = json!("complete");
            out.push(v);
        } else {
            out.push(preset.clone());
        }
    }
    Ok(json!(out))
}

pub fn reset_recommended(defaults: &Value, recommended: &Value) -> Value {
    config::effective(defaults, recommended, &json!({}))
}

#[cfg(test)]
mod tests;
