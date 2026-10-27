use serde_json::Value;

/// Merge by stable ID. An explicit tombstone survives repeated merges and upgrades.
pub fn merge(base: &Value, patch: &Value) -> Value {
    match (base, patch) {
        (Value::Object(a), Value::Object(b)) => {
            let mut out = a.clone();
            for (k, v) in b {
                out.insert(
                    k.clone(),
                    out.get(k)
                        .map(|old| merge(old, v))
                        .unwrap_or_else(|| v.clone()),
                );
            }
            Value::Object(out)
        }
        (Value::Array(a), Value::Array(b)) if a.iter().chain(b).all(|v| id(v).is_some()) => {
            let mut out = a.clone();
            for v in b {
                if let Some(i) = out.iter().position(|x| id(x) == id(v)) {
                    out[i] = merge(&out[i], v)
                } else {
                    out.push(v.clone())
                }
            }
            Value::Array(out)
        }
        (_, v) => v.clone(),
    }
}
fn id(v: &Value) -> Option<&str> {
    v.get("id")
        .or_else(|| v.get("presetId"))
        .and_then(Value::as_str)
}
pub fn materialize(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(
            a.iter()
                .filter(|v| v["_delete"] != true)
                .map(materialize)
                .collect(),
        ),
        Value::Object(o) => Value::Object(
            o.iter()
                .filter(|(k, _)| k.as_str() != "_delete")
                .map(|(k, v)| (k.clone(), materialize(v)))
                .collect(),
        ),
        _ => v.clone(),
    }
}
pub fn effective(defaults: &Value, preset: &Value, overrides: &Value) -> Value {
    materialize(&merge(&merge(defaults, preset), overrides))
}

/// Convert a full form edit into a persistent override, including deleted ID rows.
pub fn from_form(base: &Value, edited: &Value) -> Value {
    match (base, edited) {
        (Value::Object(a), Value::Object(b)) => Value::Object(
            b.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        a.get(k)
                            .map(|old| from_form(old, v))
                            .unwrap_or_else(|| v.clone()),
                    )
                })
                .collect(),
        ),
        (Value::Array(a), Value::Array(b)) if a.iter().chain(b).all(|v| id(v).is_some()) => {
            let mut out: Vec<Value> = b
                .iter()
                .map(|v| {
                    a.iter()
                        .find(|x| id(x) == id(v))
                        .map(|old| from_form(old, v))
                        .unwrap_or_else(|| v.clone())
                })
                .collect();
            for old in a {
                if !b.iter().any(|v| id(v) == id(old)) {
                    let key = if old.get("id").is_some() {
                        "id"
                    } else {
                        "presetId"
                    };
                    let mut tombstone = serde_json::Map::new();
                    tombstone.insert(key.into(), Value::String(id(old).unwrap().into()));
                    tombstone.insert("_delete".into(), Value::Bool(true));
                    out.push(Value::Object(tombstone));
                }
            }
            Value::Array(out)
        }
        (_, v) => v.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn precedence_unknown_and_tombstones() {
        let defaults =
            json!({"time":"23:00","variants":[{"id":"a","text":"a"},{"id":"b","text":"b"}]});
        let preset = json!({"time":"22:00","future":{"opaque":42}});
        let local = json!({"time":"21:00","variants":[{"id":"a","_delete":true}]});
        let v = effective(&defaults, &preset, &local);
        assert_eq!(v["time"], "21:00");
        assert_eq!(v["future"]["opaque"], 42);
        assert_eq!(v["variants"].as_array().unwrap().len(), 1);
        assert_eq!(effective(&defaults, &preset, &merge(&local, &json!({}))), v);
    }
    #[test]
    fn null_and_empty_are_explicit() {
        assert_eq!(
            merge(
                &json!({"audio":"a","items":[1]}),
                &json!({"audio":null,"items":[]})
            ),
            json!({"audio":null,"items":[]})
        );
    }
    #[test]
    fn form_deletion_does_not_resurrect_defaults() {
        let base = json!({"variants":[{"id":"a"},{"id":"b"}]});
        let edited = json!({"variants":[{"id":"b"}]});
        let patch = from_form(&base, &edited);
        assert_eq!(effective(&base, &json!({}), &patch), edited);
    }
}
