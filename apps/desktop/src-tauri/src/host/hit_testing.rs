//! Keep the bubble's layout space while transparent space passes mouse input through.
use super::*;
#[derive(Clone, Copy, Debug, Deserialize)]
pub(super) struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
impl Rect {
    fn valid(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|n| n.is_finite() && n.abs() < 32768.)
            && self.width > 0.
            && self.height > 0.
    }
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}
#[derive(Default)]
pub(super) struct Regions(pub Mutex<HitState>);
#[derive(Default)]
pub(super) struct HitState {
    pub hovered: bool,
    rects: Vec<Rect>,
    updated: Option<Instant>,
}
impl HitState {
    fn accepts(&self, x: f64, y: f64) -> bool {
        self.updated
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
            && self.rects.iter().any(|r| r.contains(x, y))
    }
}
#[cfg(feature = "smoke-test")]
pub(super) fn diagnostics(app: &tauri::AppHandle) -> Value {
    let regions = app.state::<Arc<Regions>>();
    let r = regions.0.lock().unwrap();
    json!({"regionCount":r.rects.len(),"updatedRecently":r.updated.is_some_and(|t|t.elapsed()<Duration::from_secs(3)),"regions":r.rects.iter().map(|b|json!({"x":b.x,"y":b.y,"width":b.width,"height":b.height})).collect::<Vec<_>>()})
}
pub(super) fn start(app: &tauri::AppHandle) {
    app.manage(Arc::new(Regions::default()));
    #[cfg(windows)]
    {
        let app = app.clone();
        let Some(pet) = app.get_webview_window("pet") else {
            return;
        };
        let Ok(hwnd) = pet.hwnd() else {
            return;
        };
        let hwnd = hwnd.0 as isize;
        std::thread::spawn(move || {
            #[repr(C)]
            struct Point {
                x: i32,
                y: i32,
            }
            #[link(name = "user32")]
            extern "system" {
                fn GetCursorPos(point: *mut Point) -> i32;
                fn ScreenToClient(hwnd: isize, point: *mut Point) -> i32;
            }
            let mut ignored = None;
            loop {
                let mut point = Point { x: 0, y: 0 };
                let scale = pet.scale_factor().unwrap_or(1.);
                let hit = unsafe {
                    if GetCursorPos(&mut point) != 0 && ScreenToClient(hwnd, &mut point) != 0 {
                        app.state::<Arc<Regions>>()
                            .0
                            .lock()
                            .unwrap()
                            .accepts(point.x as f64 / scale, point.y as f64 / scale)
                    } else {
                        false
                    }
                };
                app.state::<Arc<Regions>>().0.lock().unwrap().hovered = hit;
                if ignored != Some(!hit) && pet.set_ignore_cursor_events(!hit).is_ok() {
                    ignored = Some(!hit);
                }
                std::thread::sleep(Duration::from_millis(16));
            }
        });
    }
}
#[tauri::command]
pub(super) fn pet_hit_regions(
    window: WebviewWindow,
    state: State<Shared>,
    regions: State<Arc<Regions>>,
    instance_id: Option<String>,
    package_hash: Option<String>,
    rects: Vec<Rect>,
) -> Result<(), String> {
    if window.label() != "pet" {
        return Err("仅桌宠窗口可报告模型命中区域".into());
    }
    if rects.len() > 8 || rects.iter().any(|r| !r.valid()) {
        return Err("模型命中区域无效".into());
    }
    {
        let h = state.lock().unwrap();
        if h.store.active != instance_id
            || h.instance().map(|i| &i.package_hash) != package_hash.as_ref()
        {
            return Err("模型命中区域关联已失效".into());
        }
    }
    let mut r = regions.0.lock().unwrap();
    r.rects = rects;
    r.updated = Some(Instant::now());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_current_model_and_toolbar_accept_pointer() {
        let mut s = HitState {
            hovered: false,
            updated: Some(Instant::now()),
            rects: vec![
                Rect {
                    x: 100.,
                    y: 300.,
                    width: 120.,
                    height: 200.,
                },
                Rect {
                    x: 70.,
                    y: 520.,
                    width: 180.,
                    height: 36.,
                },
            ],
        };
        assert!(!s.accepts(80., 120.));
        assert!(s.accepts(120., 310.));
        assert!(s.accepts(80., 530.));
        assert!(!s.accepts(250., 530.));
        s.updated = Some(Instant::now() - Duration::from_secs(4));
        assert!(!s.accepts(120., 310.));
        assert!(!Rect {
            x: f64::NAN,
            y: 0.,
            width: 1.,
            height: 1.
        }
        .valid());
    }
}
