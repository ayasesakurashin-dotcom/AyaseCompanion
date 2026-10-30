//! Weather is a dated model estimate; location consent and data freshness are separate.
use super::{location, Settings};
use crate::services;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
pub const TTL_SECONDS: i64 = 900;
pub fn signature(s: &Settings) -> String {
    json!([
        s.weather_enabled,
        s.weather_location_mode,
        s.device_location_enabled,
        s.city,
        s.latitude,
        s.longitude
    ])
    .to_string()
}
#[cfg(test)]
pub fn fetch(s: &Settings) -> Result<Value, String> {
    request(s, &AtomicBool::new(false))
}
fn allowed(s: &Settings, cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("天气请求已取消".into());
    }
    if !s.weather_enabled {
        return Err("天气服务已关闭".into());
    }
    if s.weather_location_mode == "device"
        && (!s.device_location_enabled || location::access() != "allowed")
    {
        return Err("设备位置读取未允许，请申请 Windows 权限或改用固定地点".into());
    }
    Ok(())
}
pub fn request(s: &Settings, cancel: &AtomicBool) -> Result<Value, String> {
    s.validate()?;
    allowed(s, cancel)?;
    let fix = if s.weather_location_mode == "device" {
        Some(location::read(cancel)?)
    } else {
        None
    };
    // Only weather-grid coordinates leave the process; never upload a precise fix.
    let lat = fix
        .as_ref()
        .map(|f| f.latitude)
        .or(s.latitude)
        .ok_or("地点未配置")?;
    let lon = fix
        .as_ref()
        .map(|f| f.longitude)
        .or(s.longitude)
        .ok_or("地点未配置")?;
    let lat = (lat * 100.).round() / 100.;
    let lon = (lon * 100.).round() / 100.;
    allowed(s, cancel)?;
    if let Some(f) = &fix {
        f.validate(Utc::now())?;
    }
    let v = services::json_response(
        services::client(15)?
            .get("https://api.open-meteo.com/v1/forecast")
            .query(&[
                ("latitude", lat.to_string()),
                ("longitude", lon.to_string()),
                (
                    "current",
                    "temperature_2m,weather_code,wind_speed_10m".into(),
                ),
                ("temperature_unit", "celsius".into()),
                ("wind_speed_unit", "kmh".into()),
                ("timezone", "UTC".into()),
                ("timeformat", "unixtime".into()),
                ("forecast_days", "1".into()),
            ])
            .send(),
    )?;
    allowed(s, cancel)?;
    if let Some(f) = &fix {
        f.validate(Utc::now())?;
    }
    parse(&v, s, lat, lon, fix.as_ref(), Utc::now())
}
pub fn fresh(v: &Value, now: DateTime<Utc>) -> bool {
    ["sampledAt", "fetchedAt"].iter().all(|key| {
        v[key]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .is_some_and(|t| {
                (-60_000..TTL_SECONDS * 1000)
                    .contains(&(now - t.with_timezone(&Utc)).num_milliseconds())
            })
    })
}
pub fn refresh_after(v: &Value, now: DateTime<Utc>) -> u64 {
    let sample = v["sampledAt"]
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| TTL_SECONDS - (now - t.with_timezone(&Utc)).num_seconds())
        .unwrap_or(30);
    sample.clamp(30, TTL_SECONDS) as u64 * 1000
}
pub fn distance_km(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let h = ((c - a).to_radians() / 2.).sin().powi(2)
        + a.to_radians().cos() * c.to_radians().cos() * ((d - b).to_radians() / 2.).sin().powi(2);
    12742. * h.clamp(0., 1.).sqrt().asin()
}
fn parse(
    v: &Value,
    s: &Settings,
    lat: f64,
    lon: f64,
    fix: Option<&location::Fix>,
    now: DateTime<Utc>,
) -> Result<Value, String> {
    let response_lat = v["latitude"]
        .as_f64()
        .filter(|v| v.is_finite() && (-90.0..=90.0).contains(v))
        .ok_or("天气缺少有效地点")?;
    let response_lon = v["longitude"]
        .as_f64()
        .filter(|v| v.is_finite() && (-180.0..=180.0).contains(v))
        .ok_or("天气缺少有效地点")?;
    if distance_km(lat, lon, response_lat, response_lon) > 100. {
        return Err("天气返回地点与查询地点不匹配".into());
    }
    let sample =
        DateTime::from_timestamp(v["current"]["time"].as_i64().ok_or("天气缺少采样时间")?, 0)
            .ok_or("天气采样时间无效")?;
    let t = v["current"]["temperature_2m"]
        .as_f64()
        .filter(|n| n.is_finite() && (-90.0..=65.0).contains(n))
        .ok_or("天气温度无效")?;
    let code = v["current"]["weather_code"]
        .as_u64()
        .filter(|n| {
            [
                0, 1, 2, 3, 45, 48, 51, 53, 55, 56, 57, 61, 63, 65, 66, 67, 71, 73, 75, 77, 80, 81,
                82, 85, 86, 95, 96, 99,
            ]
            .contains(n)
        })
        .ok_or("天气代码无效")?;
    let wind = v["current"]["wind_speed_10m"]
        .as_f64()
        .filter(|v| v.is_finite() && (0.0..=500.0).contains(v))
        .ok_or("天气风速无效")?;
    if v["current_units"]["temperature_2m"] != "°C"
        || v["current_units"]["wind_speed_10m"] != "km/h"
        || v["current_units"]["time"] != "unixtime"
    {
        return Err("天气单位不匹配".into());
    }
    let result = json!({"city":if fix.is_some(){"Windows 设备附近"}else{&s.city},"latitude":lat,"longitude":lon,"locationSource":s.weather_location_mode,"locationMeasuredAt":fix.map(|f|f.measured_at.to_rfc3339()),"accuracyMeters":fix.map(|f|f.accuracy_meters),"temperature":t,"code":code,"windKmh":wind,"sampledAt":sample.to_rfc3339(),"fetchedAt":now.to_rfc3339(),"source":"https://open-meteo.com/","kind":"model-estimate"});
    if !fresh(&result, now) {
        return Err("天气采样超过15分钟或设备时间异常，未用于关怀".into());
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn response(now: DateTime<Utc>) -> Value {
        json!({"latitude":35.68,"longitude":139.69,"current":{"time":now.timestamp(),"temperature_2m":20,"weather_code":1,"wind_speed_10m":5},"current_units":{"temperature_2m":"°C","wind_speed_10m":"km/h","time":"unixtime"}})
    }
    #[test]
    fn reject_stale_future_wrong_place_and_units() {
        let now = Utc::now();
        let mut s = Settings::default();
        s.city = "Synthetic".into();
        let mut v = response(now);
        let result = parse(&v, &s, 35.68, 139.69, None, now).unwrap();
        assert!(fresh(&result, now));
        assert!(!fresh(&result, now + chrono::Duration::seconds(900)));
        for delta in [-901, 61] {
            v["current"]["time"] = json!((now + chrono::Duration::seconds(delta)).timestamp());
            assert!(parse(&v, &s, 35.68, 139.69, None, now).is_err());
        }
        v = response(now);
        assert!(parse(&v, &s, 0., 0., None, now).is_err());
        v["current_units"]["temperature_2m"] = json!("°F");
        assert!(parse(&v, &s, 35.68, 139.69, None, now).is_err());
    }
    #[test]
    fn permission_off_and_cancel_do_not_read_or_query() {
        let s = Settings {
            weather_enabled: true,
            weather_location_mode: "device".into(),
            ..Default::default()
        };
        assert!(request(&s, &AtomicBool::new(false)).is_err());
        let s = Settings {
            weather_enabled: true,
            city: "Synthetic".into(),
            latitude: Some(0.),
            longitude: Some(0.),
            ..Default::default()
        };
        assert!(request(&s, &AtomicBool::new(true)).is_err());
    }
    #[test]
    fn location_accuracy_and_timestamp_are_checked() {
        let now = Utc::now();
        let mut f = location::Fix {
            latitude: 0.,
            longitude: 0.,
            accuracy_meters: 5000.,
            measured_at: now,
        };
        f.validate(now).unwrap();
        f.accuracy_meters = 5001.;
        assert!(f.validate(now).is_err());
        f.accuracy_meters = 10.;
        f.measured_at = now - chrono::Duration::seconds(301);
        assert!(f.validate(now).is_err());
        f.measured_at = now + chrono::Duration::seconds(61);
        assert!(f.validate(now).is_err());
        f.measured_at = now;
        f.latitude = f64::NAN;
        assert!(f.validate(now).is_err());
    }
}
