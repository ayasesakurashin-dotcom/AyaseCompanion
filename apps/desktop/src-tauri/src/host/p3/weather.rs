use super::*;
use crate::p3::{location, weather};
use std::sync::atomic::{AtomicBool, Ordering};
impl Host {
    pub(in crate::host) fn weather_start_valid(&mut self, id: u64) -> bool {
        if self
            .scheduler
            .tasks
            .iter()
            .find(|t| t.id == id)
            .is_some_and(|t| t.status == ayase_core::scheduler::Status::Presenting)
        {
            return true;
        }
        let Some((signature, fact, received)) = self.p3.weather_grants.get(&id) else {
            return true;
        };
        let valid = self.current_weather(Utc::now()).is_some()
            && signature == &weather::signature(&self.ai.settings.p3)
            && self.now().saturating_sub(*received) < 900_000
            && weather::fresh(fact, Utc::now());
        if !valid {
            self.scheduler.finish(
                id,
                ayase_core::scheduler::Status::Expired,
                "天气证据已过期、配置或权限已更改",
                self.now(),
            );
        }
        valid
    }
    pub(super) fn update_weather_configuration(&mut self) {
        let signature = weather::signature(&self.ai.settings.p3);
        if self.p3.weather_settings != signature {
            self.p3.weather_cancel.store(true, Ordering::Relaxed);
            self.p3.weather_cancel = Arc::new(AtomicBool::new(false));
            self.p3.weather_settings = signature;
            self.p3.weather_rx = None;
            self.p3.weather = None;
            self.p3.weather_usable = false;
            self.p3.next_weather = 0;
            self.p3.next_access_check = 0;
            self.p3.status.remove("weather");
        }
    }
    pub(super) fn current_weather(&self, wall: DateTime<Utc>) -> Option<&Value> {
        self.p3.weather.as_ref().filter(|w| {
            self.ai.settings.p3.weather_enabled
                && self.p3.weather_usable
                && self.p3.weather_settings == weather::signature(&self.ai.settings.p3)
                && self.now().saturating_sub(self.p3.weather_received) < 900_000
                && weather::fresh(w, wall)
        })
    }
    pub(super) fn weather_snapshot(&self) -> Value {
        let settings = &self.ai.settings.p3;
        let state = if !settings.weather_enabled {
            "disabled"
        } else if settings.weather_location_mode == "device"
            && (!settings.device_location_enabled || self.p3.location_access != "allowed")
        {
            "permission-required"
        } else if self.p3.weather_rx.is_some() {
            "updating"
        } else if self.current_weather(Utc::now()).is_some() {
            "fresh"
        } else if self.p3.weather_usable {
            "stale"
        } else if self.p3.status.contains_key("weather") {
            "error"
        } else {
            "empty"
        };
        json!({"currentTime":Utc::now().to_rfc3339(),"timeZone":chrono::Local::now().offset().to_string(),"locationAccess":self.p3.location_access,"weatherState":state,"weatherUsable":self.current_weather(Utc::now()).is_some(),"message":self.p3.status.get("weather"),"ttlSeconds":900})
    }
    pub(super) fn begin_weather_refresh(&mut self) -> Result<(), String> {
        let s = self.ai.settings.p3.clone();
        if !s.weather_enabled {
            return Err("天气服务已关闭，请先启用并保存".into());
        }
        s.validate()?;
        if s.weather_location_mode == "device"
            && (!s.device_location_enabled || self.p3.location_access != "allowed")
        {
            return Err("Windows 位置读取未允许，请先在本页申请权限；也可改用固定地点".into());
        }
        if self.p3.weather_rx.is_some() {
            return Ok(());
        }
        let signature = weather::signature(&s);
        let cancel = self.p3.weather_cancel.clone();
        let (tx, rx) = mpsc::channel();
        self.p3.weather_rx = Some(rx);
        self.p3.next_weather = self.now() + 60_000;
        self.p3
            .status
            .insert("weather".into(), "正在读取地点并联网核验天气…".into());
        std::thread::spawn(move || {
            let result = weather::request(&s, &cancel);
            let _ = tx.send((signature, result));
        });
        Ok(())
    }
    pub(super) fn accept_weather(&mut self, signature: &str, result: Result<Value, String>) {
        if signature != weather::signature(&self.ai.settings.p3)
            || self.p3.weather_cancel.load(Ordering::Relaxed)
        {
            return;
        }
        match result {
            Ok(value) => {
                self.p3.next_weather = self.now() + weather::refresh_after(&value, Utc::now());
                self.p3.weather_received = self.now();
                self.p3.weather = Some(value);
                self.p3.weather_usable = true;
                self.p3.status.insert(
                    "weather".into(),
                    "天气已更新并通过地点、采样时间及单位校验（模型估算）".into(),
                );
            }
            Err(e) => {
                self.p3.weather_usable = false;
                self.p3.next_weather = self.now() + 60_000;
                self.p3.status.insert("weather".into(), e);
            }
        }
    }
    pub(super) fn weather_tick(&mut self, _wall: DateTime<Utc>) {
        self.update_weather_configuration();
        let now = self.now();
        if now >= self.p3.next_access_check {
            self.p3.next_access_check = now + 2000;
            let access = if self.ai.settings.p3.device_location_enabled {
                location::access()
            } else {
                "off"
            };
            if self.p3.location_access != access {
                self.p3.location_access = access.into();
                if self.ai.settings.p3.weather_location_mode == "device" {
                    self.p3.weather_cancel.store(true, Ordering::Relaxed);
                    self.p3.weather_cancel = Arc::new(AtomicBool::new(false));
                    self.p3.weather_rx = None;
                    self.p3.weather_usable = false;
                    self.p3.next_weather = 0;
                    if access != "allowed" {
                        self.p3.status.insert(
                            "weather".into(),
                            "Windows 定位未允许，设备天气暂停；可申请权限或改用固定地点".into(),
                        );
                    }
                }
            }
        }
        if let Some(rx) = &self.p3.weather_rx {
            match rx.try_recv() {
                Ok((signature, result)) => {
                    self.p3.weather_rx = None;
                    self.accept_weather(&signature, result);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.p3.weather_rx = None;
                    self.p3.weather_usable = false;
                    self.p3
                        .status
                        .insert("weather".into(), "天气读取未完成".into());
                }
                Err(_) => {}
            }
        }
        if now >= self.p3.next_weather
            && self.p3.weather_rx.is_none()
            && self.ai.settings.p3.weather_enabled
        {
            self.p3.next_weather = now + 60_000;
            let _ = self.begin_weather_refresh();
        }
        let pending = self.p3.weather_grants.keys().copied().collect::<Vec<_>>();
        for id in pending {
            if self
                .scheduler
                .tasks
                .iter()
                .find(|t| t.id == id)
                .is_some_and(|t| t.status.terminal())
            {
                self.p3.weather_grants.remove(&id);
            } else {
                self.weather_start_valid(id);
            }
        }
    }
}
