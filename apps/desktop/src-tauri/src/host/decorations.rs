use super::*;

impl Host {
    pub(super) fn effective_model(&self) -> Option<Value> {
        self.package
            .as_ref()
            .and_then(|p| p.model.as_ref())
            .map(|m| {
                ayase_core::decorations::effective(
                    m,
                    &self
                        .instance()
                        .map(|i| i.overrides.clone())
                        .unwrap_or_default(),
                )
            })
    }
    pub(super) fn set_decoration(&mut self, value: &Value) -> Result<(), String> {
        let instance = self.instance().ok_or("请先选择伙伴")?;
        if value["instanceId"].as_str() != Some(instance.id.as_str()) {
            return Err("伙伴已切换".into());
        }
        let id = value["id"].as_str().ok_or("缺少装饰 ID")?;
        let model = self
            .package
            .as_ref()
            .and_then(|p| p.model.as_ref())
            .ok_or("模型不可用")?;
        if !model["decorations"]
            .as_array()
            .is_some_and(|a| a.iter().any(|d| d["id"] == id))
        {
            return Err("装饰不存在".into());
        }
        let mut store = self.store.clone();
        let i = store
            .instances
            .iter_mut()
            .find(|i| i.id == instance.id)
            .unwrap();
        if value["enabled"].is_null() {
            if let Some(o) = i.overrides["decorationEnabled"].as_object_mut() {
                o.remove(id);
            }
        } else {
            let enabled = value["enabled"].as_bool().ok_or("开关需要布尔值")?;
            if !i.overrides["decorationEnabled"].is_object() {
                i.overrides["decorationEnabled"] = json!({});
            }
            i.overrides["decorationEnabled"][id] = json!(enabled);
        }
        self.commit(store)
    }
    pub(super) fn resize_scene(&mut self, app: &tauri::AppHandle) {
        let scale = self
            .package
            .as_ref()
            .and_then(|p| p.model.as_ref())
            .map(|m| {
                (360.0 / m["logicalSize"]["width"].as_f64().unwrap())
                    .min(280.0 / m["logicalSize"]["height"].as_f64().unwrap())
                    * self.store.zoom
            })
            .unwrap_or(1.0);
        self.scene_bottom = resize_pet(
            app,
            self.store.zoom,
            &self.store.appearance,
            self.scene_padding.map(|v| v * scale),
            self.scene_bottom,
        );
    }
}

pub(super) fn materialize(
    files: &mut BTreeMap<String, Vec<u8>>,
    manifest: &Value,
    overrides: &Value,
) -> Result<(), String> {
    let avatar = if manifest["avatar"]["renderer"] == "sprite.v1" {
        &manifest["avatar"]
    } else {
        &manifest["avatar"]["fallback"]
    };
    let Some(path) = avatar["descriptor"].as_str() else {
        return Ok(());
    };
    let Some(bytes) = files.get(path) else {
        return Ok(());
    };
    let model: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let model = ayase_core::decorations::effective(&model, overrides);
    files.insert(
        path.into(),
        serde_json::to_vec_pretty(&model).map_err(|e| e.to_string())?,
    );
    Ok(())
}

#[tauri::command]
pub(super) fn pet_scene_layout(
    window: WebviewWindow,
    state: State<Shared>,
    instance_id: String,
    package_hash: String,
    padding: [f64; 3],
) -> Result<f64, String> {
    if window.label() != "pet" {
        return Err("仅桌宠可报告绘制范围".into());
    }
    let mut h = state.lock().unwrap();
    let i = h.instance().ok_or("没有伙伴")?;
    if i.id != instance_id || i.package_hash != package_hash {
        return Err("模型已切换".into());
    }
    if padding
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0 || *v > 7_000_000.0)
    {
        return Err("装饰绘制范围无效".into());
    }
    h.scene_padding = padding;
    h.resize_scene(window.app_handle());
    Ok(h.scene_bottom)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switches_survive_restart_update_and_export_without_crossing_instances() {
        let root = super::super::tests::TestRoot::new();
        let mut package = package::validate(&super::super::tests::archive("0.4.1")).unwrap();
        let mut model = package.model.clone().unwrap();
        model["decorationVersion"] = json!("1.0");
        model["decorations"] = json!([{"id":"wreath","side":"foreground","order":0,"enabled":true,"source":{"type":"image","asset":"image"},"anchor":{"x":0.5,"y":1},"pivot":{"x":0.5,"y":1},"offset":{"x":0,"y":0},"width":null,"height":null,"keepAspect":true,"scale":1,"speed":1,"states":null}]);
        package
            .files
            .insert("sprite.json".into(), serde_json::to_vec(&model).unwrap());
        let mut h = Host::open(root.0.clone()).unwrap();
        let first = h
            .stage_archive(
                &package::pack(
                    package.files.clone(),
                    package.manifest.clone(),
                    "manifest.json",
                )
                .unwrap(),
            )
            .unwrap();
        activate(&mut h, &first.hash, "new").unwrap();
        let id = h.instance().unwrap().id.clone();
        h.set_decoration(&json!({"instanceId":id,"id":"wreath","enabled":false}))
            .unwrap();
        assert_eq!(
            h.effective_model().unwrap()["decorations"][0]["enabled"],
            false
        );
        assert_eq!(
            h.package.as_ref().unwrap().model.as_ref().unwrap()["decorations"][0]["enabled"],
            true
        );
        package.manifest["version"] = json!("0.4.2");
        let next = h
            .stage_archive(
                &package::pack(
                    package.files.clone(),
                    package.manifest.clone(),
                    "manifest.json",
                )
                .unwrap(),
            )
            .unwrap();
        activate(&mut h, &next.hash, "update").unwrap();
        assert_eq!(
            h.effective_model().unwrap()["decorations"][0]["enabled"],
            false
        );
        let mut files = package.files.clone();
        materialize(
            &mut files,
            &package.manifest,
            &h.instance().unwrap().overrides,
        )
        .unwrap();
        assert_eq!(
            package::json_file(&files, "sprite.json").unwrap()["decorations"][0]["enabled"],
            false
        );
        let mut fallback_files = package.files.clone();
        let mut fallback_manifest = package.manifest.clone();
        fallback_manifest["avatar"] = json!({"renderer":"future.v1","fallback":{"renderer":"sprite.v1","descriptor":"sprite.json"}});
        materialize(
            &mut fallback_files,
            &fallback_manifest,
            &h.instance().unwrap().overrides,
        )
        .unwrap();
        assert_eq!(
            package::json_file(&fallback_files, "sprite.json").unwrap()["decorations"][0]
                ["enabled"],
            false
        );
        drop(h);
        let mut h = Host::open(root.0.clone()).unwrap();
        assert_eq!(
            h.effective_model().unwrap()["decorations"][0]["enabled"],
            false
        );
        activate(&mut h, &first.hash, "new").unwrap();
        assert_eq!(
            h.effective_model().unwrap()["decorations"][0]["enabled"],
            true
        );
        assert!(h
            .set_decoration(&json!({"instanceId":id,"id":"wreath","enabled":true}))
            .is_err());
        let new_id = h.instance().unwrap().id.clone();
        h.set_decoration(&json!({"instanceId":new_id,"id":"wreath","enabled":false}))
            .unwrap();
        h.set_decoration(&json!({"instanceId":new_id,"id":"wreath","enabled":null}))
            .unwrap();
        assert_eq!(
            h.effective_model().unwrap()["decorations"][0]["enabled"],
            true
        );
    }
}
