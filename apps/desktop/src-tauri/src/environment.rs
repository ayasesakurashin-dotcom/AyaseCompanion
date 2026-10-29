use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn locked() -> bool {
    LOCKED.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn drain_events() -> Vec<String> {
    std::mem::take(&mut *EVENTS.lock().unwrap())
}
#[cfg(windows)]
fn idle_seconds() -> Option<u64> {
    #[repr(C)]
    struct Last {
        size: u32,
        time: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetLastInputInfo(info: *mut Last) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetTickCount64() -> u64;
    }
    if LOCKED.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let mut info = Last {
        size: std::mem::size_of::<Last>() as u32,
        time: 0,
    };
    unsafe {
        if GetLastInputInfo(&mut info) == 0 {
            None
        } else {
            Some((GetTickCount64() as u32).wrapping_sub(info.time) as u64 / 1000)
        }
    }
}
#[cfg(windows)]
fn session_events() {
    std::thread::spawn(|| unsafe {
        use windows::{
            core::w,
            Win32::{
                Foundation::{HWND, LPARAM, LRESULT, WPARAM},
                System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION},
                UI::WindowsAndMessaging::*,
            },
        };
        unsafe extern "system" fn message(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            let event = match (msg, w.0) {
                (WM_WTSSESSION_CHANGE, 5) => Some("system.login"),
                (WM_WTSSESSION_CHANGE, 7) => {
                    LOCKED.store(true, std::sync::atomic::Ordering::Relaxed);
                    None
                }
                (WM_WTSSESSION_CHANGE, 8) => {
                    LOCKED.store(false, std::sync::atomic::Ordering::Relaxed);
                    Some("system.unlock")
                }
                (WM_POWERBROADCAST, 7 | 18) => Some("system.resume"),
                _ => None,
            };
            if let Some(event) = event {
                let mut events = EVENTS.lock().unwrap();
                if events.len() < 32 && !events.iter().any(|e| e == event) {
                    events.push(event.into());
                }
            }
            DefWindowProcW(hwnd, msg, w, l)
        }
        let class = WNDCLASSW {
            lpfnWndProc: Some(message),
            lpszClassName: w!("AyaseEnvironmentEvents"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return;
        }
        if let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("AyaseEnvironmentEvents"),
            w!("AyaseEnvironmentEvents"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            None,
        ) {
            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    #[serde(default)]
    pub idle_seconds: Option<u64>,
    pub fullscreen: Option<bool>,
    pub media: Option<bool>,
    pub fullscreen_status: String,
    pub media_status: String,
    pub media_sources: Vec<String>,
    pub foreground_app: String,
    pub sample: u64,
}
impl Default for Facts {
    fn default() -> Self {
        Self {
            idle_seconds: None,
            fullscreen: None,
            media: None,
            fullscreen_status: "采集初始化中".into(),
            media_status: "采集初始化中".into(),
            media_sources: vec![],
            foreground_app: String::new(),
            sample: 0,
        }
    }
}
#[derive(Default)]
pub struct Settings {
    pub pet_hwnd: isize,
    pub excluded: Vec<String>,
}
pub fn start(facts: Arc<Mutex<Facts>>, settings: Arc<Mutex<Settings>>, origin: Instant) {
    #[cfg(windows)]
    session_events();
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            use windows::Media::Control::{
                GlobalSystemMediaTransportControlsSessionManager as Manager,
                GlobalSystemMediaTransportControlsSessionPlaybackStatus as Playback,
            };
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let mut manager = None;
            let mut tries = 0;
            loop {
                crate::p3::screen::track();
                if manager.is_none() && tries % 30 == 0 {
                    manager = Manager::RequestAsync().ok().and_then(|op| op.get().ok());
                }
                tries += 1;
                let (hwnd, excluded) = {
                    let s = settings.lock().unwrap();
                    (s.pet_hwnd, s.excluded.clone())
                };
                let mut f = Facts {
                    idle_seconds: idle_seconds(),
                    sample: origin.elapsed().as_millis() as u64,
                    ..Default::default()
                };
                match fullscreen(hwnd, &excluded) {
                    Ok((value, app)) => {
                        f.fullscreen = Some(value);
                        f.foreground_app = app;
                        f.fullscreen_status = "桌宠所在显示器 / 前台无标题栏全屏窗口".into()
                    }
                    Err(e) => f.fullscreen_status = e,
                }
                if let Some(m) = &manager {
                    match m.GetSessions() {
                        Ok(sessions) => {
                            let mut playing = false;
                            let mut unknown = false;
                            for i in 0..sessions.Size().unwrap_or(0) {
                                if let Ok(s) = sessions.GetAt(i) {
                                    match s.GetPlaybackInfo().and_then(|p| p.PlaybackStatus()) {
                                        Ok(p) if p == Playback::Playing => {
                                            playing = true;
                                            f.media_sources.push(
                                                s.SourceAppUserModelId()
                                                    .map(|s| s.to_string_lossy())
                                                    .unwrap_or_else(|_| "未知来源".into()),
                                            );
                                        }
                                        Ok(_) => {}
                                        Err(_) => unknown = true,
                                    }
                                }
                            }
                            f.media = if playing {
                                Some(true)
                            } else if unknown {
                                None
                            } else {
                                Some(false)
                            };
                            f.media_status = if unknown {
                                "部分媒体会话状态不可用"
                            } else {
                                "Windows GSMTC 媒体会话；不支持的播放器不会被识别"
                            }
                            .into();
                        }
                        Err(e) => {
                            f.media_status = format!("媒体能力不可用：{e}");
                            manager = None
                        }
                    }
                } else {
                    f.media_status = "Windows GSMTC 不可用，将保留基础显示".into()
                }
                *facts.lock().unwrap() = f;
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (settings, origin);
            *facts.lock().unwrap() = Facts {
                fullscreen_status: "当前平台未实现".into(),
                media_status: "当前平台未实现".into(),
                ..Default::default()
            };
        }
    });
}

#[cfg(windows)]
fn fullscreen(pet: isize, excluded: &[String]) -> Result<(bool, String), String> {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{CloseHandle, HWND, RECT},
            Graphics::{
                Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS},
                Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
            },
            System::Threading::{
                GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            UI::WindowsAndMessaging::{
                GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect,
                GetWindowThreadProcessId, IsWindowVisible, GWL_STYLE, WS_CAPTION,
            },
        },
    };
    // All buffers are stack-owned; handles are read-only and process handles are closed.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() || pet == 0 {
            return Err("前台窗口或桌宠显示器暂不可用（可能锁屏）".into());
        }
        let mut class = [0u16; 128];
        let n = GetClassNameW(hwnd, &mut class);
        let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
        if ["LockScreenInputWnd", "Credential Dialog Xaml Host"].contains(&class.as_str()) {
            return Err("系统锁屏；环境事实暂不可用".into());
        }
        if ["Progman", "WorkerW", "Shell_TrayWnd"].contains(&class.as_str())
            || !IsWindowVisible(hwnd).as_bool()
        {
            return Ok((false, "桌面".into()));
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            return Ok((false, "AyaseCompanion".into()));
        }
        let mut app = String::new();
        if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut text = [0u16; 1024];
            let mut size = text.len() as u32;
            if QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(text.as_mut_ptr()),
                &mut size,
            )
            .is_ok()
            {
                app = String::from_utf16_lossy(&text[..size as usize])
                    .rsplit('\\')
                    .next()
                    .unwrap_or("")
                    .into();
            }
            let _ = CloseHandle(process);
        }
        if excluded.iter().any(|x| x.eq_ignore_ascii_case(&app)) {
            return Ok((false, app));
        }
        let monitor = MonitorFromWindow(HWND(pet as *mut _), MONITOR_DEFAULTTONEAREST);
        if monitor.0.is_null() {
            return Err("桌宠显示器不可用".into());
        }
        if monitor != MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) {
            return Ok((false, app));
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err("显示器边界读取失败".into());
        }
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        if style & WS_CAPTION.0 != 0 {
            return Ok((false, app));
        }
        let mut rect = RECT::default();
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_err()
        {
            GetWindowRect(hwnd, &mut rect).map_err(|e| e.to_string())?;
        }
        let m = info.rcMonitor;
        let full = (rect.left - m.left).abs() <= 3
            && (rect.top - m.top).abs() <= 3
            && (rect.right - m.right).abs() <= 3
            && (rect.bottom - m.bottom).abs() <= 3;
        Ok((full, app))
    }
}
