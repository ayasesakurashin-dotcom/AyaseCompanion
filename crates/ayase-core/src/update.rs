use serde_json::{json, Map, Value};

/// Three-way field merge. IDs make additions/deletions visible independently.
/// A locally changed value (including null, disabled or a deletion) wins a conflict.
pub fn preview(old: &Value, new: &Value, local: &Value) -> Value {
    let mut changes = vec![];
    let merged = merge(old, new, local, "", &mut changes);
    json!({"merged":merged,"changes":changes})
}
/// Choices may only address an actual conflict in this preview. ID array paths
/// are stable across insertion/reordering, unlike numeric JSON pointers.
pub fn choose(report: &Value, paths: &[String]) -> Result<Value, String> {
    let mut out = report["merged"].clone();
    for path in paths {
        let row = report["changes"]
            .as_array()
            .ok_or("缺少更新预览")?
            .iter()
            .find(|r| r["path"] == *path && r["kind"] == "conflict-local-kept")
            .ok_or("选择不是本次冲突")?;
        let keys = path
            .trim_start_matches('/')
            .split('/')
            .map(|p| p.replace("~1", "/").replace("~0", "~"))
            .collect::<Vec<_>>();
        fn set(v: &mut Value, keys: &[String], next: &Value, remove: bool) -> Result<(), String> {
            if keys.is_empty() {
                *v = next.clone();
                return Ok(());
            }
            let key = &keys[0];
            if let Some(a) = v.as_array_mut() {
                let i = a
                    .iter()
                    .position(|v| {
                        v.get("id")
                            .or_else(|| v.get("presetId"))
                            .and_then(Value::as_str)
                            == Some(key)
                    })
                    .ok_or("更新条目已不存在")?;
                if remove && keys.len() == 1 {
                    a.remove(i);
                    return Ok(());
                }
                return set(&mut a[i], &keys[1..], next, remove);
            }
            let o = v.as_object_mut().ok_or("更新路径不是对象")?;
            if keys.len() == 1 {
                if remove {
                    o.remove(key);
                } else {
                    o.insert(key.clone(), next.clone());
                }
                Ok(())
            } else {
                set(
                    o.get_mut(key).ok_or("更新路径缺失")?,
                    &keys[1..],
                    next,
                    remove,
                )
            }
        }
        if path.is_empty() {
            out = row["new"].clone();
        } else {
            set(&mut out, &keys, &row["new"], row["newMissing"] == true)?;
        }
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflict_choices_use_ids_and_distinguish_removed_from_null() {
        let report = preview(
            &json!({"rows":[{"id":"a/b","text":"old"}],"gone":1}),
            &json!({"rows":[{"id":"a/b","text":"new"}]}),
            &json!({"rows":[{"id":"a/b","text":"mine"}],"gone":2}),
        );
        let selected = choose(&report, &["/rows/a~1b/text".into(), "/gone".into()]).unwrap();
        assert_eq!(selected["rows"][0]["text"], "new");
        assert!(selected.get("gone").is_none());
        assert!(choose(&report, &["/not-a-conflict".into()]).is_err());
    }
    #[test]
    fn local_deletion_and_disabled_audio_survive_new_recommendations() {
        let old = json!({"rows":[{"id":"a","text":"old"},{"id":"b","text":"keep"}],"audio":{"enabled":true},"fresh":1});
        let new = json!({"rows":[{"id":"a","text":"changed"},{"id":"b","text":"new"},{"id":"c","text":"added"}],"audio":{"enabled":true},"fresh":2});
        let local = json!({"rows":[{"id":"b","text":"mine"}],"audio":{"enabled":false},"fresh":1});
        let r = preview(&old, &new, &local);
        assert_eq!(r["merged"]["audio"]["enabled"], false);
        assert_eq!(
            r["merged"]["rows"],
            json!([{"id":"b","text":"mine"},{"id":"c","text":"added"}])
        );
        assert_eq!(r["merged"]["fresh"], 2);
    }
}
fn merge(old: &Value, new: &Value, local: &Value, path: &str, changes: &mut Vec<Value>) -> Value {
    if old == new {
        return local.clone();
    }
    if local == old {
        changes.push(json!({"path":path,"kind":"template","old":old,"new":new,"local":local}));
        return new.clone();
    }
    if local == new {
        return local.clone();
    }
    if let (Some(o), Some(n), Some(l)) = (old.as_object(), new.as_object(), local.as_object()) {
        let mut out = Map::new();
        let keys: std::collections::BTreeSet<_> =
            o.keys().chain(n.keys()).chain(l.keys()).collect();
        for k in keys {
            let p = format!("{}/{}", path, k.replace('~', "~0").replace('/', "~1"));
            match (o.get(k), n.get(k), l.get(k)) {
                (_, _, None) => {
                    if o.contains_key(k) {
                        changes.push(json!({"path":p,"kind":"local-delete"}));
                    } else if let Some(v) = n.get(k) {
                        out.insert(k.clone(), v.clone());
                    }
                }
                (None, None, Some(v)) => {
                    out.insert(k.clone(), v.clone());
                }
                (Some(v), None, Some(lv)) if v == lv => {
                    changes.push(json!({"path":p,"kind":"template-delete"}));
                }
                (Some(v), None, Some(lv)) => {
                    changes.push(json!({"path":p,"kind":"conflict-local-kept","old":v,"new":null,"newMissing":true,"local":lv}));
                    out.insert(k.clone(), lv.clone());
                }
                (_, nv, Some(lv)) => {
                    let v = merge(
                        o.get(k).unwrap_or(&Value::Null),
                        nv.unwrap_or(&Value::Null),
                        lv,
                        &p,
                        changes,
                    );
                    out.insert(k.clone(), v);
                }
            }
        }
        return Value::Object(out);
    }
    let id = |v: &Value| {
        v.get("id")
            .or_else(|| v.get("presetId"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    if let (Some(o), Some(n), Some(l)) = (old.as_array(), new.as_array(), local.as_array()) {
        if o.iter().chain(n).chain(l).all(|v| id(v).is_some()) {
            let object = |a: &Vec<Value>| {
                Value::Object(a.iter().map(|v| (id(v).unwrap(), v.clone())).collect())
            };
            let mapped = merge(&object(o), &object(n), &object(l), path, changes);
            let mut order: Vec<_> = l.iter().chain(n).filter_map(id).collect();
            let mut seen = std::collections::BTreeSet::new();
            order.retain(|k| seen.insert(k.clone()));
            return Value::Array(
                order
                    .iter()
                    .filter_map(|k| mapped.get(k).cloned())
                    .collect(),
            );
        }
    }
    changes
        .push(json!({"path":path,"kind":"conflict-local-kept","old":old,"new":new,"local":local}));
    local.clone()
}
