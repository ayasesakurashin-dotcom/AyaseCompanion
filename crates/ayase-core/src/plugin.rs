use crate::{config, package::json_file, scheduler::Priority, schema};
use chrono::{DateTime, Datelike, Local, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const CAPABILITIES: &[&str] = &[
    "clock.schedule",
    "presentation.request",
    "environment.read",
    "base.claim",
    "base.release",
    "state.set",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    pub manifest: Value,
    pub schema: Value,
    pub rules: Value,
}
#[derive(Debug, Clone)]
pub enum Output {
    Present(Value, u64),
    Claim(String, bool),
    Release,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Runtime {
    pub occurrences: BTreeMap<String, String>,
    pub state: BTreeMap<String, Value>,
    #[serde(skip)]
    pub last_wall: Option<i64>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Configured {
    pub owner: String,
    pub plugin_id: String,
    pub name: String,
    pub config: Value,
    pub priority: Priority,
    pub enabled: bool,
    pub status: String,
    pub revision: u64,
    pub schema: Value,
}

pub fn validate_package(m: &Value, files: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let schema_path = m["configSchema"].as_str().ok_or("configSchema 缺失")?;
    let s = json_file(files, schema_path)?;
    schema::check_schema(&s)?;
    schema::validate(&s, &m["defaults"])?;
    let rules = json_file(files, m["runtime"]["entry"].as_str().ok_or("缺规则路径")?)?;
    let p = Plugin {
        manifest: m.clone(),
        schema: s,
        rules,
    };
    validate_plugin(&p)?;
    p.validate_config(&m["defaults"])
}
pub fn validate_plugin(p: &Plugin) -> Result<(), String> {
    schema::validate(&schema::contract("plugin"), &p.manifest)?;
    schema::validate(&schema::contract("rules"), &p.rules)?;
    let caps = p.manifest["capabilities"].as_array().unwrap();
    let cap = |c: &str| caps.iter().any(|v| v == c);
    let mut ids = BTreeSet::new();
    for sub in p.rules["subscriptions"].as_array().unwrap() {
        if !ids.insert(sub["id"].as_str().unwrap()) {
            return Err("订阅 id 重复".into());
        }
        match sub["type"].as_str().unwrap() {
            "schedule.daily" if !cap("clock.schedule") => {
                return Err("未声明 clock.schedule 能力".into())
            }
            "environment.fullscreen" | "environment.media" if !cap("environment.read") => {
                return Err("未声明 environment.read 能力".into())
            }
            _ => {}
        }
    }
    let mut rule_ids = BTreeSet::new();
    for rule in p.rules["rules"].as_array().unwrap() {
        if !rule_ids.insert(rule["id"].as_str().unwrap()) {
            return Err("规则 id 重复".into());
        }
        if !ids.contains(rule["on"].as_str().unwrap()) {
            return Err("规则引用未知订阅".into());
        }
        for action in rule["actions"].as_array().unwrap() {
            if !cap(action["type"].as_str().unwrap()) {
                return Err(format!("动作未获声明能力：{}", action["type"]));
            }
        }
    }
    Ok(())
}
fn ptr<'a>(config: &'a Value, v: &Value) -> Result<&'a Value, String> {
    let path = v.as_str().ok_or("缺少配置引用")?;
    if !path.starts_with('/') || path.len() > 256 {
        return Err("配置引用必须是受限 JSON Pointer".into());
    }
    config
        .pointer(path)
        .ok_or_else(|| format!("配置引用不存在：{path}"))
}
pub fn valid_time(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5
        && b[2] == b':'
        && b[..2].iter().chain(&b[3..]).all(u8::is_ascii_digit)
        && s[..2].parse::<u8>().unwrap_or(99) < 24
        && s[3..].parse::<u8>().unwrap_or(99) < 60
}
pub fn validate_response(response: &Value) -> Result<(), String> {
    schema::validate(&schema::contract("request"), response)?;
    let mut ids = BTreeSet::new();
    for v in response["variants"].as_array().unwrap() {
        if !ids.insert(v["id"].as_str().unwrap()) {
            return Err("回复变体 id 重复".into());
        }
        if v["text"].as_str().is_some_and(|s| !s.is_empty())
            && v["textAsset"].as_str().is_some_and(|s| !s.is_empty())
        {
            return Err("text 与 textAsset 必须二选一".into());
        }
    }
    Ok(())
}
impl Plugin {
    pub fn from_files(m: Value, files: &BTreeMap<String, Vec<u8>>) -> Result<Self, String> {
        validate_package(&m, files)?;
        Ok(Self {
            schema: json_file(files, m["configSchema"].as_str().unwrap())?,
            rules: json_file(files, m["runtime"]["entry"].as_str().unwrap())?,
            manifest: m,
        })
    }
    pub fn validate_config(&self, c: &Value) -> Result<(), String> {
        schema::validate(&self.schema, c)?;
        if let Some(tz) = c["timeZone"].as_str() {
            if tz != "system" && tz.parse::<chrono_tz::Tz>().is_err() {
                return Err("timeZone: 未知 IANA 时区".into());
            }
        }
        for sub in self.rules["subscriptions"].as_array().unwrap() {
            if sub["type"] == "schedule.daily" {
                let time = ptr(c, &sub["timeFromConfig"])?
                    .as_str()
                    .ok_or("时间必须为文本")?;
                if !valid_time(time) {
                    return Err("time: 需要 HH:mm".into());
                }
                let tz = ptr(c, &sub["timeZoneFromConfig"])?
                    .as_str()
                    .ok_or("时区必须为文本")?;
                if tz != "system" && tz.parse::<chrono_tz::Tz>().is_err() {
                    return Err("timeZone: 未知 IANA 时区".into());
                }
                if !["skip", "within-ttl"].contains(
                    &ptr(c, &sub["missedPolicyFromConfig"])?
                        .as_str()
                        .unwrap_or(""),
                ) {
                    return Err("missedPolicy 只允许 skip / within-ttl".into());
                }
            }
        }
        for rule in self.rules["rules"].as_array().unwrap() {
            for a in rule["actions"].as_array().unwrap() {
                match a["type"].as_str().unwrap() {
                    "presentation.request" => {
                        let response = ptr(c, &a["responseFromConfig"])?;
                        if response.is_array() {
                            for w in response.as_array().unwrap() {
                                for key in ["start", "end"] {
                                    if !valid_time(w[key].as_str().unwrap_or("")) {
                                        return Err(format!("windows/{key}: 无效时刻"));
                                    }
                                }
                                validate_response(&w["response"])?;
                            }
                        } else {
                            validate_response(response)?;
                        }
                        let ttl = ptr(c, &a["ttlSecondsFromConfig"])?
                            .as_u64()
                            .ok_or("ttlSeconds 必须为整数")?;
                        if !(1..=3600).contains(&ttl) {
                            return Err("ttlSeconds 范围为 1—3600".into());
                        }
                    }
                    "base.claim" => {
                        if ptr(c, &a["modeFromConfig"])?
                            .as_str()
                            .filter(|s| !s.is_empty() && s.len() < 64)
                            .is_none()
                        {
                            return Err("基础姿态名称无效".into());
                        }
                        if !ptr(c, &a["suppressFromConfig"])?.is_boolean() {
                            return Err("suppress 需要布尔值".into());
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
    pub fn configure(&self, preset: &Value, patch: &Value, revision: u64) -> Configured {
        let empty = serde_json::json!({});
        let defaults = if preset["configMode"] == "complete" {
            &empty
        } else {
            &self.manifest["defaults"]
        };
        let effective = config::effective(
            defaults,
            preset.get("config").unwrap_or(&empty),
            patch.get("config").unwrap_or(&empty),
        );
        let enabled = patch["enabled"]
            .as_bool()
            .or_else(|| preset["enabled"].as_bool())
            .unwrap_or(false);
        let priority = patch["priority"]
            .as_str()
            .or_else(|| preset["priority"].as_str())
            .or_else(|| self.manifest["defaultPriority"].as_str())
            .unwrap_or("normal");
        let validation = self
            .validate_config(&effective)
            .and_then(|_| Priority::plugin(priority).map(|_| ()));
        let missing: Vec<_> = self.manifest["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|c| !CAPABILITIES.contains(c))
            .collect();
        let status = if preset["configVersion"] != self.manifest["configVersion"] {
            "配置版本不兼容，原记录保留".into()
        } else if !missing.is_empty() {
            format!("缺少必需能力：{}", missing.join(", "))
        } else if let Err(e) = validation {
            format!("配置无效：{e}")
        } else if enabled {
            "已启用".into()
        } else {
            "已关闭".into()
        };
        Configured {
            owner: preset["presetId"].as_str().unwrap_or("").into(),
            plugin_id: self.manifest["pluginId"].as_str().unwrap().into(),
            name: self.manifest["displayName"].as_str().unwrap().into(),
            config: effective,
            priority: Priority::plugin(priority).unwrap_or(Priority::Normal),
            enabled: enabled && status == "已启用",
            status,
            revision,
            schema: self.schema.clone(),
        }
    }
    pub fn dispatch(
        &self,
        c: &Configured,
        event: &str,
        active: Option<bool>,
        now: DateTime<Utc>,
        runtime: &mut Runtime,
    ) -> Vec<Output> {
        if !c.enabled {
            return vec![];
        }
        let mut out = vec![];
        for sub in self.rules["subscriptions"].as_array().unwrap() {
            let typ = sub["type"].as_str().unwrap();
            let id = sub["id"].as_str().unwrap();
            if (event == "clock.tick" && typ != "schedule.daily")
                || (event != "clock.tick" && typ != event)
            {
                continue;
            }
            if typ == "schedule.daily" {
                let time = ptr(&c.config, &sub["timeFromConfig"])
                    .unwrap()
                    .as_str()
                    .unwrap();
                let tz = ptr(&c.config, &sub["timeZoneFromConfig"])
                    .unwrap()
                    .as_str()
                    .unwrap();
                let (date, hour, minute, second) = wall_parts(now, tz);
                let current = hour * 3600 + minute * 60 + second;
                let due = time[..2].parse::<u32>().unwrap() * 3600
                    + time[3..].parse::<u32>().unwrap() * 60;
                let policy = ptr(&c.config, &sub["missedPolicyFromConfig"])
                    .unwrap()
                    .as_str()
                    .unwrap();
                let ttl = c.config["ttlSeconds"].as_u64().unwrap_or(300) as u32;
                let gap = runtime
                    .last_wall
                    .map(|last| now.timestamp() - last)
                    .unwrap_or(0);
                let eligible = current >= due
                    && current - due < if policy == "within-ttl" { ttl } else { 60 }
                    && (policy == "within-ttl" || gap <= 90);
                let occurrence = format!("{tz}:{date}:{time}");
                let key = format!("{}:{id}", c.owner);
                if !eligible || runtime.occurrences.get(&key) == Some(&occurrence) {
                    continue;
                }
                runtime.occurrences.insert(key, occurrence);
            }
            for rule in self.rules["rules"].as_array().unwrap() {
                if rule["on"] != id {
                    continue;
                }
                match rule["when"].as_str().unwrap_or("always") {
                    "active" if active != Some(true) => continue,
                    "inactive" if active != Some(false) => continue,
                    _ => {}
                }
                for a in rule["actions"].as_array().unwrap() {
                    match a["type"].as_str().unwrap() {
                        "presentation.request" => {
                            let r = ptr(&c.config, &a["responseFromConfig"]).unwrap();
                            let ttl = ptr(&c.config, &a["ttlSecondsFromConfig"])
                                .unwrap()
                                .as_u64()
                                .unwrap()
                                * 1000;
                            if let Some(windows) = r.as_array() {
                                let tz = c.config["timeZone"].as_str().unwrap_or("system");
                                let (_, h, m, _) = wall_parts(now, tz);
                                let current = h * 60 + m;
                                if let Some(w) = windows.iter().find(|w| {
                                    let parse = |key: &str| {
                                        let t = w[key].as_str().unwrap();
                                        t[..2].parse::<u32>().unwrap() * 60
                                            + t[3..].parse::<u32>().unwrap()
                                    };
                                    let start = parse("start");
                                    let end = parse("end");
                                    w["allDay"] == true
                                        || if start <= end {
                                            current >= start && current < end
                                        } else {
                                            current >= start || current < end
                                        }
                                }) {
                                    out.push(Output::Present(w["response"].clone(), ttl));
                                }
                            } else {
                                out.push(Output::Present(r.clone(), ttl))
                            }
                        }
                        "base.claim" => out.push(Output::Claim(
                            ptr(&c.config, &a["modeFromConfig"])
                                .unwrap()
                                .as_str()
                                .unwrap()
                                .into(),
                            ptr(&c.config, &a["suppressFromConfig"])
                                .unwrap()
                                .as_bool()
                                .unwrap(),
                        )),
                        "base.release" => out.push(Output::Release),
                        "state.set" => {
                            let key =
                                format!("{}:{}", c.owner, a["key"].as_str().unwrap_or("state"));
                            if runtime.state.len() < 256 || runtime.state.contains_key(&key) {
                                runtime.state.insert(key, a["value"].clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        out
    }
}
pub fn wall_parts(now: DateTime<Utc>, tz: &str) -> (String, u32, u32, u32) {
    fn parts<T: TimeZone>(d: DateTime<T>) -> (String, u32, u32, u32) {
        (
            format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day()),
            d.hour(),
            d.minute(),
            d.second(),
        )
    }
    if tz == "system" {
        parts(now.with_timezone(&Local))
    } else {
        parts(now.with_timezone(&tz.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC)))
    }
}
pub fn choose_variant(response: &Value, unit: f64) -> Option<Value> {
    let a = response["variants"].as_array()?;
    let sum: f64 = a.iter().map(|v| v["weight"].as_f64().unwrap_or(1.0)).sum();
    let mut target = unit.clamp(0.0, 0.999999999) * sum;
    for v in a {
        target -= v["weight"].as_f64().unwrap_or(1.0);
        if target < 0.0 {
            return Some(v.clone());
        }
    }
    a.last().cloned()
}
pub fn builtin() -> BTreeMap<String, Plugin> {
    let values: [Value; 3] = [
        serde_json::from_str(include_str!("../../../plugins/builtin/startup.json")).unwrap(),
        serde_json::from_str(include_str!("../../../plugins/builtin/fullscreen.json")).unwrap(),
        serde_json::from_str(include_str!("../../../plugins/builtin/media.json")).unwrap(),
    ];
    values
        .into_iter()
        .map(|v| {
            let p: Plugin = serde_json::from_value(v).unwrap();
            (p.manifest["pluginId"].as_str().unwrap().into(), p)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn builtin_contracts_and_capabilities() {
        for p in builtin().values() {
            validate_plugin(p).unwrap();
            p.validate_config(&p.manifest["defaults"]).unwrap();
        }
    }
    #[test]
    fn valid_times() {
        for t in ["00:00", "23:59"] {
            assert!(valid_time(t))
        }
        for t in ["24:00", "23:60", "9:00", "hello"] {
            assert!(!valid_time(t))
        }
    }
    #[test]
    fn pairs_stay_together() {
        let r = json!({"variants":[{"id":"a","text":"A","audioAsset":"A"},{"id":"b","text":"B","audioAsset":"B"}]});
        for u in [0.0, 0.8] {
            let v = choose_variant(&r, u).unwrap();
            assert_eq!(v["text"], v["audioAsset"]);
        }
    }
    #[test]
    fn unknown_config_stays_disabled() {
        let p = builtin().remove("ayase.startup-greeting").unwrap();
        let c = p.configure(
            &json!({"presetId":"x","configVersion":"2.0","enabled":true,"config":{}}),
            &json!({"config":{}}),
            0,
        );
        assert!(!c.enabled);
    }
}
