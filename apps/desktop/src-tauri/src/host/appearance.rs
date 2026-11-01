use super::*;
use ayase_core::theme;
impl Host {
    pub(super) fn appearance_snapshot(&self) -> Value {
        json!({"settings":self.store.appearance,"assetPaths":self.appearance_assets})
    }
    pub(super) fn load_appearance_assets(&mut self) {
        self.appearance_assets.clear();
        if let Some(hash) = &self.store.appearance_source {
            if let Ok(p) = self.load_package(hash) {
                for (id, a) in &self.store.appearance.assets {
                    if let Some(path) = a["path"]
                        .as_str()
                        .filter(|path| p.files.contains_key(*path))
                    {
                        self.appearance_assets.insert(
                            id.clone(),
                            self.root
                                .join("packages")
                                .join(hash)
                                .join("content")
                                .join(path)
                                .to_string_lossy()
                                .into(),
                        );
                    }
                }
            }
        }
    }
}
#[tauri::command]
pub(super) fn appearance_settings(
    window: WebviewWindow,
    state: State<Shared>,
    action: String,
    value: Value,
) -> Result<(), String> {
    control(&window)?;
    let mut h = state.lock().unwrap();
    let (definition, source, files) = match action.as_str() {
        "apply-package" => {
            let hash = value.as_str().ok_or("缺少主题包")?;
            let p = h.load_package(hash)?;
            if p.report.kind != "theme" {
                return Err("请选择主题包".into());
            }
            (p.manifest, Some(hash.to_owned()), p.files)
        }
        "use-recommended" => {
            let p = h.package.as_ref().ok_or("没有当前伙伴")?;
            let path = p.manifest["recommendedTheme"]
                .as_str()
                .ok_or("此角色包没有推荐主题")?;
            (
                package::json_file(&p.files, path)?,
                Some(p.report.hash.clone()),
                p.files.clone(),
            )
        }
        "reset" => (theme::defaults(), None, BTreeMap::new()),
        "save" => {
            let files = h
                .store
                .appearance_source
                .as_ref()
                .and_then(|hash| h.load_package(hash).ok())
                .map(|p| p.files)
                .unwrap_or_default();
            (value, h.store.appearance_source.clone(), files)
        }
        _ => return Err("未知主题操作".into()),
    };
    let appearance = theme::parse(&definition, &files)?;
    let mut store = h.store.clone();
    store.bubble = BubblePreferences {
        animation: appearance.animation.clone(),
        characters_per_second: appearance.characters_per_second,
        font_size: appearance.font_size,
        read_ms: appearance.read_ms,
    };
    store.appearance = appearance;
    store.appearance_source = source;
    h.commit(store)?;
    h.load_appearance_assets();
    if let Some(hash) = &h.store.appearance_source {
        let _ = window
            .app_handle()
            .asset_protocol_scope()
            .allow_directory(h.root.join("packages").join(hash).join("content"), true);
    }
    resize_pet(window.app_handle(), h.store.zoom, &h.store.appearance);
    Ok(())
}
