use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Optional visual layers never become finite presentation channels.
pub fn validate(
    model: &Value,
    dimensions: &BTreeMap<String, (u32, u32)>,
) -> Result<Vec<String>, String> {
    let Some(layers) = model.get("decorations").and_then(Value::as_array) else {
        return Ok(vec![]);
    };
    if model["decorationVersion"] != "1.0" {
        return Err("装饰配置需要 decorationVersion 1.0".into());
    }
    let mut ids = BTreeSet::new();
    let mut warnings = vec![];
    for layer in layers {
        let id = layer["id"].as_str().ok_or("装饰缺少 ID")?;
        if !ids.insert(id) {
            return Err(format!("装饰 ID 重复：{id}"));
        }
        if let Some(states) = layer["states"].as_array() {
            let mut unique = BTreeSet::new();
            if states
                .iter()
                .any(|s| !unique.insert(s.as_str().unwrap_or_default()))
            {
                return Err(format!("装饰 {id} 状态重复"));
            }
        }
        let source = &layer["source"];
        let asset = if source["type"] == "image" {
            source["asset"].as_str()
        } else {
            source["clip"]
                .as_str()
                .and_then(|c| model["clips"][c]["asset"].as_str())
        };
        if !asset.is_some_and(|id| dimensions.contains_key(id)) {
            warnings.push(format!("装饰 {id} 缺少有效图像或片段，跳过该层"));
        }
    }
    Ok(warnings)
}

pub fn effective(model: &Value, overrides: &Value) -> Value {
    let mut result = model.clone();
    if let Some(layers) = result.get_mut("decorations").and_then(Value::as_array_mut) {
        for layer in layers {
            if let Some(enabled) = layer["id"]
                .as_str()
                .and_then(|id| overrides["decorationEnabled"][id].as_bool())
            {
                layer["enabled"] = json!(enabled);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn many_layers_and_independent_overrides() {
        let mut model = json!({"decorationVersion":"1.0","decorations":[]});
        model["decorations"]=json!((0..80).map(|i|json!({"id":format!("layer-{i}"),"source":{"type":"image","asset":"image"},"states":null,"enabled":true})).collect::<Vec<_>>());
        let dims = BTreeMap::from([("image".into(), (64, 64))]);
        assert!(validate(&model, &dims).unwrap().is_empty());
        let modified = effective(&model, &json!({"decorationEnabled":{"layer-79":false}}));
        assert_eq!(modified["decorations"][79]["enabled"], false);
        assert_eq!(model["decorations"][79]["enabled"], true);
        assert_eq!(modified["decorations"][0]["enabled"], true);
        model["decorations"][79]["id"] = json!("layer-0");
        assert!(validate(&model, &dims).is_err());
    }
    #[test]
    fn optional_missing_resources_do_not_block_model() {
        let m = json!({"decorationVersion":"1.0","decorations":[{"id":"optional","source":{"type":"clip","clip":"missing"},"states":null}]});
        assert_eq!(validate(&m, &BTreeMap::new()).unwrap().len(), 1);
        assert!(validate(&json!({"decorations":[]}), &BTreeMap::new()).is_err());
    }
}
