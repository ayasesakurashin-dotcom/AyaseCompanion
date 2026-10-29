//! Ephemeral targets; selecting metadata never captures pixels.
use super::Settings;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
    time::{Duration, Instant},
};
static LAST: Mutex<Option<isize>> = Mutex::new(None);
type Watches = BTreeMap<isize, Vec<Weak<AtomicBool>>>;
static WATCHED: OnceLock<Mutex<Watches>> = OnceLock::new();
static READY: AtomicBool = AtomicBool::new(false);
fn watched() -> &'static Mutex<Watches> {
    WATCHED.get_or_init(Mutex::default)
}
#[derive(Clone)]
pub struct Target {
    pub app: Option<tauri::AppHandle>,
    handle: isize,
    pid: u32,
    started: u64,
    class: String,
    alive: Arc<AtomicBool>,
    prepared: Instant,
    pub scope: String,
    pub label: String,
    region: [i32; 4],
    excluded: Vec<String>,
    pub masks: Vec<[i32; 4]>,
}
pub struct Capture {
    pub image: Vec<u8>,
    pub observed_at: String,
    pub scope: String,
    pub label: String,
}
pub fn track() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let hwnd = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 && pid != std::process::id() {
            *LAST.lock().unwrap() = Some(hwnd.0 as isize);
        }
    }
}
pub fn invalidate() {
    for flags in watched().lock().unwrap().values() {
        for flag in flags.iter().filter_map(Weak::upgrade) {
            flag.store(false, Ordering::SeqCst);
        }
    }
    watched().lock().unwrap().clear();
}
pub fn start(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::HWND,
            UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK},
        };
        unsafe extern "system" fn destroyed(
            _: HWINEVENTHOOK,
            _: u32,
            hwnd: HWND,
            object: i32,
            child: i32,
            _: u32,
            _: u32,
        ) {
            if object == 0 && child == 0 {
                if let Some(flags) = watched().lock().unwrap().remove(&(hwnd.0 as isize)) {
                    for f in flags.iter().filter_map(Weak::upgrade) {
                        f.store(false, Ordering::SeqCst);
                    }
                }
            }
        }
        let _ = app.run_on_main_thread(|| unsafe {
            let hook = SetWinEventHook(0x8001, 0x8001, None, Some(destroyed), 0, 0, 0);
            READY.store(!hook.0.is_null(), Ordering::SeqCst);
        });
    }
    #[cfg(not(windows))]
    let _ = app;
}
#[cfg(windows)]
fn identity(handle: isize) -> Result<(u32, u64, String, String, [i32; 4]), String> {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{CloseHandle, FILETIME, HWND, RECT},
            System::Threading::{
                GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            UI::WindowsAndMessaging::{
                GetClassNameW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindow,
                IsWindowVisible,
            },
        },
    };
    unsafe {
        let hwnd = HWND(handle as *mut _);
        if !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool()
        {
            return Err("所选窗口已不可见，请重新选择".into());
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return Err("请选择外部应用窗口".into());
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .map_err(|_| "窗口身份不可读")?;
        let (mut a, mut b, mut c, mut d) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        let result = GetProcessTimes(process, &mut a, &mut b, &mut c, &mut d);
        let mut path = [0u16; 2048];
        let mut len = path.len() as u32;
        let name_result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        result.map_err(|_| "窗口身份不可读")?;
        name_result.map_err(|_| "窗口应用不可读")?;
        let label = String::from_utf16_lossy(&path[..len as usize])
            .rsplit('\\')
            .next()
            .unwrap_or("外部应用")
            .to_owned();
        let mut class = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut class);
        let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
        if [
            "Progman",
            "WorkerW",
            "Shell_TrayWnd",
            "LockScreenInputWnd",
            "Credential Dialog Xaml Host",
        ]
        .contains(&class.as_str())
        {
            return Err("当前窗口不适合采集".into());
        }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).map_err(|_| "窗口范围不可读")?;
        Ok((
            pid,
            (a.dwHighDateTime as u64) << 32 | a.dwLowDateTime as u64,
            class,
            label,
            [
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ],
        ))
    }
}
pub fn prepare(s: &Settings) -> Result<Target, String> {
    if !s.screen_enabled || !s.vision.enabled {
        return Err("请先启用屏幕采集并配置视觉服务".into());
    }
    #[cfg(windows)]
    {
        if !READY.load(Ordering::SeqCst) {
            return Err("窗口生命周期监听尚未就绪".into());
        }
        track();
        let handle = LAST
            .lock()
            .unwrap()
            .ok_or("尚未找到外部应用，请先切到目标窗口再选择")?;
        let (pid, started, class, label, _) = identity(handle)?;
        if s.excluded_windows
            .iter()
            .any(|v| label.to_lowercase().contains(&v.to_lowercase()))
        {
            return Err("该应用在屏幕排除列表中".into());
        }
        let alive = Arc::new(AtomicBool::new(true));
        watched()
            .lock()
            .unwrap()
            .entry(handle)
            .or_default()
            .push(Arc::downgrade(&alive));
        Ok(Target {
            app: None,
            handle,
            pid,
            started,
            class,
            alive,
            prepared: Instant::now(),
            scope: s.capture_scope.clone(),
            label,
            region: s.capture_region,
            excluded: s.excluded_windows.clone(),
            masks: vec![],
        })
    }
    #[cfg(not(windows))]
    {
        Err("屏幕采集仅在 Windows 实现".into())
    }
}
impl Target {
    pub fn valid(&self) -> bool {
        self.alive.load(Ordering::SeqCst) && self.prepared.elapsed() < Duration::from_secs(600)
    }
}
pub fn capture(t: &Target) -> Result<Capture, String> {
    if !t.valid() || crate::environment::locked() {
        return Err("屏幕目标已失效，请重新选择".into());
    }
    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::{HWND, RECT},
            Graphics::Gdi::{
                GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
            },
            UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        };
        let (pid, started, class, label, mut rect) = identity(t.handle)?;
        if pid != t.pid
            || started != t.started
            || class != t.class
            || t.excluded
                .iter()
                .any(|v| label.to_lowercase().contains(&v.to_lowercase()))
        {
            return Err("屏幕目标已改变，请重新选择".into());
        }
        unsafe {
            let fg = GetForegroundWindow();
            let mut current = 0;
            GetWindowThreadProcessId(fg, Some(&mut current));
            if current != 0 && current != std::process::id() && fg.0 as isize != t.handle {
                return Err("外部前台窗口已改变，请重新选择目标".into());
            }
            if t.scope == "screen" {
                let m = MonitorFromWindow(HWND(t.handle as *mut _), MONITOR_DEFAULTTONEAREST);
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if !GetMonitorInfoW(m, &mut info).as_bool() {
                    return Err("显示器不可用".into());
                }
                let RECT {
                    left,
                    top,
                    right,
                    bottom,
                } = info.rcMonitor;
                rect = [left, top, right - left, bottom - top];
            }
        }
        if t.scope == "region" {
            rect = t.region;
        }
        let mut image = pixels(rect)?;
        use tauri::Manager;
        let masks = t
            .app
            .as_ref()
            .map(|app| {
                app.webview_windows()
                    .values()
                    .filter(|w| w.is_visible().unwrap_or(false))
                    .filter_map(|w| {
                        Some({
                            let p = w.outer_position().ok()?;
                            let s = w.outer_size().ok()?;
                            [p.x, p.y, s.width as i32, s.height as i32]
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| t.masks.clone());
        for [x, y, w, h] in &masks {
            let left = x.saturating_sub(rect[0]).clamp(0, rect[2]) as u32;
            let top = y.saturating_sub(rect[1]).clamp(0, rect[3]) as u32;
            let right = x
                .saturating_add(*w)
                .saturating_sub(rect[0])
                .clamp(0, rect[2]) as u32;
            let bottom = y
                .saturating_add(*h)
                .saturating_sub(rect[1])
                .clamp(0, rect[3]) as u32;
            for py in top..bottom {
                for px in left..right {
                    image.put_pixel(px, py, image::Rgb([32, 32, 32]));
                }
            }
        }
        if !t.valid()
            || crate::environment::locked()
            || !identity(t.handle).is_ok_and(|(pid, started, class, _, _)| {
                pid == t.pid && started == t.started && class == t.class
            })
        {
            return Err("采集时目标已关闭".into());
        }
        if image
            .pixels()
            .step_by(79)
            .all(|p| p.0.iter().all(|v| *v < 5))
        {
            return Err("画面为空或受到保护".into());
        }
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .thumbnail(1600, 1200)
            .write_to(&mut out, image::ImageFormat::Png)
            .map_err(|_| "画面编码失败")?;
        Ok(Capture {
            image: out.into_inner(),
            observed_at: chrono::Utc::now().to_rfc3339(),
            scope: t.scope.clone(),
            label: t.label.clone(),
        })
    }
    #[cfg(not(windows))]
    Err("屏幕采集仅在 Windows 实现".into())
}
#[cfg(windows)]
fn pixels(r: [i32; 4]) -> Result<image::RgbImage, String> {
    use windows::Win32::Graphics::Gdi::*;
    let [x, y, w, h] = r;
    if w <= 0 || h <= 0 || w > 16384 || h > 16384 || w as u64 * h as u64 > 32_000_000 {
        return Err("截图范围过大或无效".into());
    }
    static LOCK: Mutex<()> = Mutex::new(());
    let _lock = LOCK.lock().unwrap();
    unsafe {
        let dc = GetDC(None);
        if dc.is_invalid() {
            return Err("无法获取画面".into());
        }
        let mem = CreateCompatibleDC(dc);
        if mem.is_invalid() {
            ReleaseDC(None, dc);
            return Err("无法创建画面缓存".into());
        }
        let bmp = CreateCompatibleBitmap(dc, w, h);
        if bmp.is_invalid() {
            let _ = DeleteDC(mem);
            ReleaseDC(None, dc);
            return Err("画面缓存分配失败".into());
        }
        let old = SelectObject(mem, bmp);
        let result = (|| {
            BitBlt(mem, 0, 0, w, h, dc, x, y, SRCCOPY).map_err(|_| "画面复制失败")?;
            let stride = (w as usize * 3 + 3) & !3;
            let mut bytes = vec![0u8; stride * h as usize];
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 24,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            SelectObject(mem, old);
            let n = GetDIBits(
                dc,
                bmp,
                0,
                h as u32,
                Some(bytes.as_mut_ptr() as *mut _),
                &mut info,
                DIB_RGB_COLORS,
            );
            if n != h {
                return Err("画面读取不完整".into());
            }
            let mut rgb = image::RgbImage::new(w as u32, h as u32);
            for py in 0..h as u32 {
                for px in 0..w as u32 {
                    let i = py as usize * stride + px as usize * 3;
                    rgb.put_pixel(px, py, image::Rgb([bytes[i + 2], bytes[i + 1], bytes[i]]));
                }
            }
            Ok(rgb)
        })();
        SelectObject(mem, old);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        ReleaseDC(None, dc);
        result
    }
}
pub fn base64(bytes: &[u8]) -> String {
    const MAP: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (c.get(1).copied().unwrap_or(0) as u32) << 8
            | c.get(2).copied().unwrap_or(0) as u32;
        out.push(MAP[((n >> 18) & 63) as usize] as char);
        out.push(MAP[((n >> 12) & 63) as usize] as char);
        out.push(if c.len() > 1 {
            MAP[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            MAP[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
