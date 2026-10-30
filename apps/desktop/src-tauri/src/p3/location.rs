//! One-shot Windows fixes, only after explicit app consent. Never use IP fallback.
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
mod runtime;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fix {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy_meters: f64,
    pub measured_at: DateTime<Utc>,
}
impl Fix {
    pub fn validate(&self, now: DateTime<Utc>) -> Result<(), String> {
        if !self.latitude.is_finite()
            || !(-90.0..=90.0).contains(&self.latitude)
            || !self.longitude.is_finite()
            || !(-180.0..=180.0).contains(&self.longitude)
        {
            return Err("Windows 位置坐标无效".into());
        }
        if !self.accuracy_meters.is_finite() || !(0.0..=5000.0).contains(&self.accuracy_meters) {
            return Err("Windows 位置精度不足（需不超过5公里）；请改用固定地点".into());
        }
        let age = (now - self.measured_at).num_seconds();
        if !(-60..=300).contains(&age) {
            return Err("Windows 位置已过期或设备时间异常，请重新读取".into());
        }
        Ok(())
    }
}
pub fn access() -> &'static str {
    #[cfg(windows)]
    {
        runtime::access()
    }
    #[cfg(not(windows))]
    {
        "unavailable"
    }
}
pub fn read(cancel: &AtomicBool) -> Result<Fix, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("位置读取已取消".into());
    }
    if access() != "allowed" {
        return Err(
            "Windows 位置权限未允许。请在本页申请权限，并检查系统的定位服务与桌面应用权限".into(),
        );
    }
    #[cfg(windows)]
    {
        runtime::read(cancel)
    }
    #[cfg(not(windows))]
    {
        Err("此系统不支持 Windows 设备位置".into())
    }
}

/// Called on the foreground UI thread; waiting happens on a worker.
#[cfg(windows)]
pub fn request_access() -> Result<
    windows::Foundation::IAsyncOperation<windows::Devices::Geolocation::GeolocationAccessStatus>,
    String,
> {
    runtime::request_access()
}
#[cfg(windows)]
pub fn wait<T: windows::core::RuntimeType + Send + 'static>(
    op: windows::Foundation::IAsyncOperation<T>,
    seconds: u64,
    cancel: &AtomicBool,
) -> Result<T, String> {
    runtime::wait(op, seconds, cancel)
}
