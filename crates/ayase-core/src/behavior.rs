//! Built-in behavior policies. Clocks and facts are supplied by the host; no I/O.
use crate::plugin::{wall_parts, Configured};
use chrono::{DateTime, Datelike, Utc};
use serde_json::{json, Value};
use std::collections::BTreeMap;
#[cfg(test)]
mod tests;

#[derive(Default, Clone)]
pub struct Input {
    pub monotonic_ms: u64,
    pub idle_seconds: Option<u64>,
    pub manual_work: bool,
    pub manual_rest: bool,
    pub pointer: Option<(String, String)>,
    pub weather: Option<Value>,
    pub calendar: Vec<Value>,
    pub special: Vec<Value>,
}
#[derive(Clone, Debug)]
pub struct Candidate {
    pub response: Value,
    pub facts: Value,
    pub observe: bool,
    pub proactive: bool,
    pub key: String,
}
pub fn in_window(minute: u32, start: &str, end: &str) -> bool {
    let parse =
        |s: &str| s[..2].parse::<u32>().unwrap_or(0) * 60 + s[3..].parse::<u32>().unwrap_or(0);
    if !crate::plugin::valid_time(start) || !crate::plugin::valid_time(end) {
        return false;
    }
    let (a, b) = (parse(start), parse(end));
    if a == b {
        true
    } else if a < b {
        minute >= a && minute < b
    } else {
        minute >= a || minute < b
    }
}
pub fn validate(kind: &str, c: &Value) -> Result<(), String> {
    if kind.is_empty() {
        return Ok(());
    }
    if kind == "scheduled-reminder" && !crate::plugin::valid_time(c["time"].as_str().unwrap_or(""))
    {
        return Err("提醒时间需要 HH:mm".into());
    }
    crate::plugin::validate_response(&c["response"])?;
    if !crate::plugin::valid_time(c["start"].as_str().unwrap_or(""))
        || !crate::plugin::valid_time(c["end"].as_str().unwrap_or(""))
    {
        return Err("互动时段需要 HH:mm".into());
    }
    if kind == "random-interaction" && c["minSeconds"].as_u64() > c["maxSeconds"].as_u64() {
        return Err("最短间隔不能大于最长间隔".into());
    }
    if kind == "weather-care" && c["lowC"].as_f64() >= c["highC"].as_f64() {
        return Err("低温阈值须小于高温阈值".into());
    }
    Ok(())
}
/// State keys include owner and configuration revision. Disabling removes live timers.
pub fn evaluate(
    kind: &str,
    c: &Configured,
    input: &Input,
    wall: DateTime<Utc>,
    unit: f64,
    state: &mut BTreeMap<String, Value>,
) -> Option<Candidate> {
    if !c.enabled || kind.is_empty() {
        return None;
    }
    let k = format!("p3:{}:{}", c.owner, c.revision);
    let s = state.entry(k).or_insert_with(
        || json!({"last":input.monotonic_ms,"next":input.monotonic_ms,"work":0,"rest":0,"count":0}),
    );
    let last = s["last"].as_u64().unwrap_or(input.monotonic_ms);
    let step = input.monotonic_ms.saturating_sub(last).min(3000);
    s["last"] = json!(input.monotonic_ms);
    let tz = c.config["timeZone"].as_str().unwrap_or("system");
    let (date, h, m, _) = wall_parts(wall, tz);
    if s["date"] != date {
        s["date"] = json!(date);
        s["count"] = json!(0);
    }
    let mut facts = Value::Null;
    let due = match kind {
        "scheduled-reminder" => {
            if c.config["repeat"] == "interval" {
                if s["scheduled"] != true {
                    s["scheduled"] = json!(true);
                    s["next"] = json!(
                        input.monotonic_ms
                            + c.config["intervalSeconds"].as_u64().unwrap_or(3600) * 1000
                    );
                    false
                } else {
                    input.monotonic_ms >= s["next"].as_u64().unwrap_or(u64::MAX)
                }
            } else {
                let time = c.config["time"].as_str().unwrap_or("23:00");
                let due = time[..2].parse::<u32>().unwrap_or(23) * 60
                    + time[3..].parse::<u32>().unwrap_or(0);
                let day = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                    .unwrap()
                    .weekday()
                    .number_from_monday();
                let allowed = c.config["repeat"] != "weekdays"
                    || c.config["days"]
                        .as_array()
                        .is_some_and(|d| d.contains(&json!(day)));
                let gap = s["lastWall"]
                    .as_i64()
                    .map(|t| wall.timestamp() - t)
                    .unwrap_or(0);
                s["lastWall"] = json!(wall.timestamp());
                let sec = (h * 60 + m) * 60 + crate::plugin::wall_parts(wall, tz).3;
                let valid_seconds = if c.config["missedPolicy"] == "within-ttl" {
                    c.config["ttlSeconds"].as_u64().unwrap_or(180)
                } else {
                    60
                };
                facts = json!({"date":date,"time":time,"repeat":c.config["repeat"]});
                allowed
                    && sec >= due * 60
                    && (sec - due * 60) < valid_seconds as u32
                    && (c.config["missedPolicy"] == "within-ttl" || gap <= 90)
            }
        }
        "work-reminder" => {
            let resting = input.manual_rest
                || input
                    .idle_seconds
                    .is_some_and(|v| v >= c.config["restSeconds"].as_u64().unwrap_or(300));
            if resting {
                s["work"] = json!(0);
                false
            } else {
                let active = match c.config["basis"].as_str().unwrap_or("app-runtime") {
                    "manual" => input.manual_work,
                    "device-active" => input.idle_seconds.is_some_and(|n| {
                        n < c.config["activeWithinSeconds"].as_u64().unwrap_or(60)
                    }),
                    _ => true,
                };
                if active {
                    s["work"] = json!(s["work"].as_u64().unwrap_or(0) + step);
                }
                facts = json!({"basis":c.config["basis"],"countedSeconds":s["work"].as_u64().unwrap_or(0)/1000,"note":"所选口径的累计时间，不证明持续工作"});
                s["work"].as_u64().unwrap_or(0)
                    >= c.config["thresholdSeconds"].as_u64().unwrap_or(3600) * 1000
            }
        }
        "random-interaction" => {
            if s["scheduled"] != true {
                schedule(s, c, input.monotonic_ms, unit);
                false
            } else {
                input.monotonic_ms >= s["next"].as_u64().unwrap_or(u64::MAX)
            }
        }
        "pointer-interaction" => input.pointer.as_ref().is_some_and(|(click, area)| {
            click == c.config["click"].as_str().unwrap_or("double")
                && (c.config["area"].as_str().unwrap_or("").is_empty()
                    || area == c.config["area"].as_str().unwrap_or(""))
        }),
        "observation" => {
            if s["scheduled"] != true {
                s["scheduled"] = json!(true);
                s["next"] = json!(
                    input.monotonic_ms + c.config["intervalSeconds"].as_u64().unwrap_or(900) * 1000
                );
                false
            } else {
                input.monotonic_ms >= s["next"].as_u64().unwrap_or(u64::MAX)
            }
        }
        "weather-care" => input.weather.as_ref().is_some_and(|w| {
            facts = w.clone();
            let t = w["temperature"].as_f64().unwrap_or(20.);
            let code = w["code"].as_u64().unwrap_or(0);
            t <= c.config["lowC"].as_f64().unwrap_or(5.)
                || t >= c.config["highC"].as_f64().unwrap_or(32.)
                || (c.config["rain"] == true && matches!(code,51..=86|95..=99))
        }),
        "calendar-care" => {
            let allowed = c.config["categories"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let items = input
                .calendar
                .iter()
                .filter(|v| allowed.contains(&v["category"]))
                .cloned()
                .collect::<Vec<_>>();
            facts = json!(items);
            !facts.as_array().unwrap().is_empty()
        }
        "special-date" => {
            facts = json!(input.special);
            !input.special.is_empty()
        }
        "routine-care" => input
            .idle_seconds
            .is_some_and(|v| v < c.config["activeWithinSeconds"].as_u64().unwrap_or(300)),
        _ => false,
    };
    let proactive = kind != "pointer-interaction";
    let minute = h * 60 + m;
    if !due
        || (proactive
            && !in_window(
                minute,
                c.config["start"].as_str().unwrap_or("00:00"),
                c.config["end"].as_str().unwrap_or("00:00"),
            ))
    {
        return None;
    }
    if s["count"].as_u64().unwrap_or(0) >= c.config["dailyLimit"].as_u64().unwrap_or(2)
        || s["acceptedWall"].as_i64().is_some_and(|t| {
            wall.timestamp() - t < c.config["cooldownSeconds"].as_u64().unwrap_or(2700) as i64
        })
        || s["accepted"].as_u64().is_some_and(|n| {
            input.monotonic_ms < n + c.config["cooldownSeconds"].as_u64().unwrap_or(2700) * 1000
        })
    {
        return None;
    }
    let key = format!(
        "{kind}:{date}:{}",
        if kind == "weather-care" {
            format!(
                "{}:{}:{}",
                facts["city"],
                facts["code"],
                if facts["temperature"].as_f64().unwrap_or(20.)
                    <= c.config["lowC"].as_f64().unwrap_or(5.)
                {
                    "cold"
                } else if facts["temperature"].as_f64().unwrap_or(20.)
                    >= c.config["highC"].as_f64().unwrap_or(32.)
                {
                    "hot"
                } else {
                    "rain"
                }
            )
        } else if matches!(
            kind,
            "calendar-care" | "special-date" | "scheduled-reminder"
        ) {
            facts.to_string()
        } else {
            String::new()
        }
    );
    if matches!(kind, "weather-care" | "calendar-care" | "special-date")
        || (kind == "scheduled-reminder" && c.config["repeat"] != "interval")
    {
        if s["factKey"] == key {
            return None;
        }
    }
    Some(Candidate {
        response: c.config["response"].clone(),
        facts,
        observe: kind == "observation",
        proactive,
        key,
    })
}
fn schedule(s: &mut Value, c: &Configured, now: u64, unit: f64) {
    let min = c.config["minSeconds"].as_u64().unwrap_or(1800);
    let max = c.config["maxSeconds"].as_u64().unwrap_or(min);
    s["next"] = json!(now + (min + ((max - min) as f64 * unit.clamp(0., 0.999999)) as u64) * 1000);
    s["scheduled"] = json!(true);
}
pub fn accepted(
    c: &Configured,
    kind: &str,
    key: &str,
    now: u64,
    unit: f64,
    state: &mut BTreeMap<String, Value>,
) {
    let s = state
        .entry(format!("p3:{}:{}", c.owner, c.revision))
        .or_insert_with(|| json!({}));
    s["accepted"] = json!(now);
    s["count"] = json!(s["count"].as_u64().unwrap_or(0) + 1);
    s["factKey"] = json!(key);
    if kind == "random-interaction" {
        schedule(s, c, now, unit);
    }
    if kind == "observation" {
        s["next"] = json!(now + c.config["intervalSeconds"].as_u64().unwrap_or(900) * 1000);
    }
    if kind == "scheduled-reminder" && c.config["repeat"] == "interval" {
        s["next"] = json!(now + c.config["intervalSeconds"].as_u64().unwrap_or(3600) * 1000);
    }
    if kind == "work-reminder" {
        s["work"] = json!(0);
    }
}
