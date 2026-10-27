use serde_json::Value;

// Versioned, closed JSON Schema subset. Unsupported validation keywords are rejected,
// never silently ignored. No remote references, script evaluation or network access.
const KEYS: &[&str] = &[
    "$schema",
    "$id",
    "$defs",
    "$ref",
    "title",
    "description",
    "default",
    "examples",
    "type",
    "const",
    "enum",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "minItems",
    "maxItems",
    "minimum",
    "maximum",
    "minLength",
    "maxLength",
    "pattern",
    "anyOf",
    "oneOf",
];
pub fn check_schema(s: &Value) -> Result<(), String> {
    check(s, 0)
}
fn check(s: &Value, depth: usize) -> Result<(), String> {
    if depth > 24 {
        return Err("Schema 嵌套过深".into());
    }
    if s.is_boolean() {
        return Ok(());
    }
    let o = s.as_object().ok_or("Schema 必须为对象或布尔值")?;
    for k in o.keys() {
        if !KEYS.contains(&k.as_str()) {
            return Err(format!("不支持的 Schema 关键字：{k}"));
        }
    }
    if let Some(r) = s["$ref"].as_str() {
        if !r.starts_with("#/$defs/") {
            return Err("只允许本文件 $defs 引用".into());
        }
    }
    if let Some(t) = s["type"].as_str() {
        if ![
            "object", "array", "string", "number", "integer", "boolean", "null",
        ]
        .contains(&t)
        {
            return Err("未知 Schema 类型".into());
        }
    }
    if let Some(p) = s["pattern"].as_str() {
        if p.len() > 512 {
            return Err("正则过长".into());
        }
        regex::Regex::new(p).map_err(|e| e.to_string())?;
    }
    for key in ["properties", "$defs"] {
        if let Some(props) = s[key].as_object() {
            for v in props.values() {
                check(v, depth + 1)?
            }
        }
    }
    for key in ["items", "additionalProperties"] {
        if let Some(v) = s.get(key) {
            check(v, depth + 1)?
        }
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(arr) = s[key].as_array() {
            if arr.len() > 16 {
                return Err("Schema 分支过多".into());
            }
            for v in arr {
                check(v, depth + 1)?
            }
        }
    }
    Ok(())
}
pub fn validate(s: &Value, v: &Value) -> Result<(), String> {
    check_schema(s)?;
    walk(s, s, v, "$", 0, &mut 0)
}
fn walk(
    root: &Value,
    s: &Value,
    v: &Value,
    path: &str,
    depth: usize,
    steps: &mut usize,
) -> Result<(), String> {
    *steps += 1;
    if depth > 32 || *steps > 100_000 {
        return Err("校验复杂度超限".into());
    }
    let err = |msg: &str| Err(format!("{path}: {msg}"));
    if s == &Value::Bool(true) {
        return Ok(());
    }
    if s == &Value::Bool(false) {
        return err("此值不被允许");
    }
    if let Some(r) = s["$ref"].as_str() {
        let sub = root.pointer(&r[1..]).ok_or_else(|| format!("未找到 {r}"))?;
        return walk(root, sub, v, path, depth + 1, steps);
    }
    if let Some(t) = s["type"].as_str() {
        let ok = match t {
            "object" => v.is_object(),
            "array" => v.is_array(),
            "string" => v.is_string(),
            "number" => v.is_number(),
            "integer" => v.as_i64().is_some() || v.as_u64().is_some(),
            "boolean" => v.is_boolean(),
            "null" => v.is_null(),
            _ => false,
        };
        if !ok {
            return err(&format!("需要 {t}"));
        }
    }
    if let Some(c) = s.get("const") {
        if c != v {
            return err("不符合固定值");
        }
    }
    if let Some(a) = s["enum"].as_array() {
        if !a.contains(v) {
            return err("不在允许选项内");
        }
    }
    for (key, exact) in [("anyOf", false), ("oneOf", true)] {
        if let Some(a) = s[key].as_array() {
            let n = a
                .iter()
                .filter(|x| walk(root, x, v, path, depth + 1, steps).is_ok())
                .count();
            if n == 0 || (exact && n != 1) {
                return err("不符合 Schema 分支");
            }
        }
    }
    if let Some(n) = v.as_f64() {
        if s["minimum"].as_f64().is_some_and(|min| n < min)
            || s["maximum"].as_f64().is_some_and(|max| n > max)
        {
            return err("数值超出范围");
        }
    }
    if let Some(t) = v.as_str() {
        let n = t.chars().count() as u64;
        if s["minLength"].as_u64().is_some_and(|m| n < m)
            || s["maxLength"].as_u64().is_some_and(|m| n > m)
        {
            return err("文本长度超限");
        }
        if let Some(p) = s["pattern"].as_str() {
            if !regex::Regex::new(p).map_err(|e| e.to_string())?.is_match(t) {
                return err("文本格式无效");
            }
        }
    }
    if let Some(a) = v.as_array() {
        let n = a.len() as u64;
        if s["minItems"].as_u64().is_some_and(|m| n < m)
            || s["maxItems"].as_u64().is_some_and(|m| n > m)
        {
            return err("列表长度超限");
        }
        if let Some(sub) = s.get("items") {
            for (i, item) in a.iter().enumerate() {
                walk(root, sub, item, &format!("{path}/{i}"), depth + 1, steps)?
            }
        }
    }
    if let Some(o) = v.as_object() {
        if let Some(required) = s["required"].as_array() {
            for k in required {
                let k = k.as_str().ok_or("required 必须是字段名")?;
                if !o.contains_key(k) {
                    return err(&format!("缺少字段 {k}"));
                }
            }
        }
        for (k, item) in o {
            if let Some(sub) = s["properties"].get(k) {
                walk(root, sub, item, &format!("{path}/{k}"), depth + 1, steps)?
            } else if let Some(sub) = s.get("additionalProperties") {
                walk(root, sub, item, &format!("{path}/{k}"), depth + 1, steps)?
            }
        }
    }
    Ok(())
}
pub fn contract(name: &str) -> Value {
    let source = match name {
        "character" => include_str!("../../../contracts/character.schema.json"),
        "sprite" => include_str!("../../../contracts/sprite.schema.json"),
        "plugin" => include_str!("../../../contracts/plugin.schema.json"),
        "rules" => include_str!("../../../contracts/rules.schema.json"),
        "theme" => include_str!("../../../contracts/theme.schema.json"),
        "request" => include_str!("../../../contracts/request.schema.json"),
        _ => "false",
    };
    serde_json::from_str(source).expect("bundled contract")
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn invalid_time_and_remote_refs() {
        assert!(validate(
            &json!({"type":"string","pattern":"^([01][0-9]|2[0-3]):[0-5][0-9]$"}),
            &json!("25:99")
        )
        .is_err());
        assert!(check_schema(&json!({"$ref":"https://evil.invalid/schema"})).is_err());
        assert!(check_schema(&json!({"format":"unknown"})).is_err());
    }
}
