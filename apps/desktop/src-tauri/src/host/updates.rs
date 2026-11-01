use super::*;
use ayase_core::update;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PackageState {
    overrides: Value,
    revisions: BTreeMap<String, u64>,
    profile: Option<conversation::Profile>,
}
impl PackageState {
    pub(super) fn capture(i: &Instance) -> Self {
        Self {
            overrides: i.overrides.clone(),
            revisions: i.revisions.clone(),
            profile: i.profile.clone(),
        }
    }
}
fn template(h: &Host, p: &Package) -> Value {
    let mut presets = serde_json::Map::new();
    for v in p.manifest["pluginPresets"].as_array().into_iter().flatten() {
        let mut row = v.clone();
        if let Some(plugin) = h.plugins.get(v["pluginId"].as_str().unwrap_or("")) {
            row["config"] = plugin.configure(v, &json!({}), 0).config;
            row["configMode"] = json!("complete");
        }
        if let Some(id) = v["presetId"].as_str() {
            presets.insert(id.into(), row);
        }
    }
    let mut identity = p.manifest["identity"].clone();
    if let Some(text) = identity["personaAsset"]
        .as_str()
        .and_then(|a| p.asset_text(a))
    {
        identity["persona"] = json!(text);
        identity.as_object_mut().unwrap().remove("personaAsset");
    }
    json!({"identity":identity,"presets":presets,"assetEnabled":p.manifest["assets"].as_object().map(|a|a.iter().map(|(id,v)|(id.clone(),json!(v["enabled"]!=false))).collect::<serde_json::Map<_,_>>())})
}

#[cfg(test)]
mod tests {
    use super::super::tests::{archive, TestRoot};
    use super::*;
    fn character(version: &str, text: &str) -> Vec<u8> {
        let mut p = ayase_core::studio::Project::from_package(
            package::validate(&archive(version)).unwrap(),
        )
        .unwrap();
        let plug = ayase_core::studio::Project::rule_plugin();
        p.document.manifest["identity"] =
            json!({"characterName":"伙伴","userAddress":"你","persona":"原始人格"});
        let mut config = plug.document.manifest["defaults"].clone();
        config["response"]["variants"] = json!([{"id":"a","text":text},{"id":"b","text":"第二项"}]);
        p.document.manifest["pluginPresets"] = json!([{"presetId":"daily","pluginId":"example.nightly-rest","configMode":"complete","configVersion":"1.0","enabled":true,"priority":"normal","config":config}]);
        p.export().unwrap()
    }
    #[test]
    fn update_merge_preserves_deletion_mute_local_values_and_rollback_snapshot() {
        let root = TestRoot::new();
        let mut h = Host::open(root.0.clone()).unwrap();
        let plugin = ayase_core::studio::Project::rule_plugin()
            .validated()
            .unwrap();
        h.plugins.insert(
            plugin.report.id,
            Plugin::from_files(plugin.manifest, &plugin.files).unwrap(),
        );
        let first = h.stage_archive(&character("0.1.0", "旧推荐")).unwrap();
        activate(&mut h, &first.hash, "new").unwrap();
        let owner = h.instance().unwrap().id.clone();
        let mut store = h.store.clone();
        let i = &mut store.instances[0];
        i.overrides["plugins"]["daily"] = json!({"enabled":false,"config":{"response":{"variants":[{"id":"a","text":"用户文字"},{"id":"b","_delete":true}]}}});
        i.overrides["assetEnabled"] = json!({"image":false});
        h.commit(store).unwrap();
        let before = h.instance().unwrap().overrides.clone();
        let next = h.stage_archive(&character("0.2.0", "新推荐")).unwrap();
        let p = h.load_package(&next.hash).unwrap();
        let report = preview(&h, &p).unwrap();
        assert!(report["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "conflict-local-kept"));
        let (overrides, _) = merged_overrides(&h, &p, &[]).unwrap();
        assert_eq!(overrides["plugins"]["daily"]["enabled"], false);
        assert_eq!(overrides["assetEnabled"]["image"], false);
        activate(&mut h, &next.hash, "update").unwrap();
        let c = &h.configured()[0];
        assert_eq!(
            c.config["response"]["variants"],
            json!([{"id":"a","text":"用户文字"}])
        );
        assert_eq!(h.instance().unwrap().id, owner);
        rollback(&mut h, &first.hash).unwrap();
        assert_eq!(h.instance().unwrap().overrides, before);
        assert_eq!(h.package.as_ref().unwrap().report.version, "0.1.0");
    }
    #[test]
    fn preview_fence_changes_on_user_configuration_and_instance_switch() {
        let root = TestRoot::new();
        let mut h = Host::open(root.0.clone()).unwrap();
        let p = h.stage_archive(&archive("0.1.0")).unwrap();
        activate(&mut h, &p.hash, "new").unwrap();
        let token = fence(&h, "new");
        h.instance_mut().unwrap().profile_revision += 1;
        assert_ne!(token, fence(&h, "new"));
        let token = fence(&h, "new");
        activate(&mut h, &p.hash, "new").unwrap();
        assert_ne!(token, fence(&h, "new"));
    }
}
fn preview(h: &Host, new: &Package) -> Result<Value, String> {
    let i = h.instance().ok_or("没有当前伙伴")?;
    let old = h.package.as_ref().ok_or("当前包未载入")?;
    if old.report.id != new.report.id {
        return Err("packageId 不同，请创建新伙伴".into());
    }
    let old_template = template(h, old);
    let mut local = old_template.clone();
    let profile = h.profile();
    local["identity"] = json!({"characterName":profile.character_name,"userAddress":profile.user_address,"persona":profile.persona,"language":profile.language});
    // Do not invent absent defaults as user overrides.
    for key in ["characterName", "userAddress", "persona", "language"] {
        if old_template["identity"].get(key).is_none() && i.overrides["identity"].get(key).is_none()
        {
            local["identity"].as_object_mut().unwrap().remove(key);
        }
    }
    let mut rows = serde_json::Map::new();
    for mut p in h.presets() {
        let id = p["presetId"].as_str().unwrap().to_owned();
        if let Some(c) = h.configured().iter().find(|c| c.owner == id) {
            p["config"] = c.config.clone();
            p["configMode"] = json!("complete");
            p["enabled"] = json!(c.enabled);
            p["priority"] = json!(c.priority);
        } else if let Some(patch) = i.overrides["plugins"].get(&id) {
            p = config::effective(&p, &json!({}), patch);
        }
        rows.insert(id, p);
    }
    local["presets"] = json!(rows);
    local["assetEnabled"] = config::effective(
        &old_template["assetEnabled"],
        &json!({}),
        &i.overrides["assetEnabled"],
    );
    Ok(update::preview(&old_template, &template(h, new), &local))
}
fn fence(h: &Host, new_hash: &str) -> String {
    let i = h.instance().unwrap();
    package::digest(
        serde_json::to_string(&json!([
            i.id,
            i.package_hash,
            new_hash,
            i.overrides,
            i.revisions,
            i.profile_revision,
            i.name
        ]))
        .unwrap()
        .as_bytes(),
    )
}
pub(super) fn merged_overrides(
    h: &Host,
    new: &Package,
    choices: &[String],
) -> Result<(Value, Option<conversation::Profile>), String> {
    let selected = update::choose(&preview(h, new)?, choices)?;
    for row in selected["presets"]
        .as_object()
        .ok_or("配置清单无效")?
        .values()
    {
        if let Some(plugin) = h.plugins.get(row["pluginId"].as_str().unwrap_or("")) {
            if row["configVersion"] == plugin.manifest["configVersion"] {
                plugin.validate_config(&row["config"])?;
            }
        }
    }
    let base = template(h, new);
    let mut overrides = h.instance().unwrap().overrides.clone();
    let mut patches = serde_json::Map::new();
    let mut extra = vec![];
    let mut removed = vec![];
    for (id, row) in selected["presets"].as_object().ok_or("配置清单无效")? {
        if let Some(original) = base["presets"].get(id) {
            patches.insert(id.clone(), config::from_form(original, row));
        } else {
            extra.push(row.clone());
        }
    }
    for id in base["presets"].as_object().unwrap().keys() {
        if selected["presets"].get(id).is_none() {
            removed.push(json!(id));
        }
    }
    overrides["plugins"] = json!(patches);
    overrides["extraPresets"] = json!(extra);
    overrides["removedPresets"] = json!(removed);
    overrides["assetEnabled"] = selected["assetEnabled"].clone();
    overrides["identity"] = selected["identity"].clone();
    let mut profile = h.profile();
    let ident = &selected["identity"];
    if let Some(v) = ident["characterName"].as_str() {
        profile.character_name = v.into();
    }
    if let Some(v) = ident["userAddress"].as_str() {
        profile.user_address = v.into();
    }
    if let Some(v) = ident["persona"].as_str() {
        profile.persona = v.into();
    }
    if let Some(v) = ident["language"].as_str() {
        profile.language = v.into();
    }
    profile.validate()?;
    Ok((overrides, Some(profile)))
}
pub(super) fn rollback(h: &mut Host, hash: &str) -> Result<(), String> {
    let p = h.load_package(hash)?;
    let mut store = h.store.clone();
    let i = store
        .instances
        .iter_mut()
        .find(|i| Some(&i.id) == store.active.as_ref())
        .ok_or("没有当前伙伴")?;
    let saved = i.rollback_states.get(hash).cloned();
    i.rollback_states
        .insert(i.package_hash.clone(), PackageState::capture(i));
    let current = i.package_hash.clone();
    i.package_hash = hash.into();
    i.previous_packages.pop();
    i.previous_packages.push(current);
    if let Some(saved) = saved {
        i.overrides = saved.overrides;
        i.revisions = saved.revisions;
        i.profile = saved.profile;
    }
    i.profile_revision += 1;
    let id = i.id.clone();
    h.commit(store)?;
    h.cancel_generations();
    let now = h.now();
    h.scheduler.switch_instance(&id, now);
    h.package = Some(p);
    h.previous_facts = (None, None);
    h.renderer_ready = false;
    h.last_wall = 0;
    Ok(())
}
#[tauri::command]
pub(super) fn package_update(
    window: WebviewWindow,
    state: State<Shared>,
    hash: String,
    action: String,
    token: Option<String>,
    choices: Option<Vec<String>>,
) -> Result<Value, String> {
    control(&window)?;
    let mut h = state.lock().unwrap();
    let p = h.load_package(&hash)?;
    if p.report.kind != "character" {
        return Err("请选择角色包".into());
    }
    let report = preview(&h, &p)?;
    let current = fence(&h, &hash);
    if action == "preview" {
        return Ok(
            json!({"token":current,"report":report,"old":h.package.as_ref().unwrap().report,"new":p.report}),
        );
    }
    if action != "apply" || token.as_deref() != Some(&current) {
        return Err("伙伴或配置已改变，请重新预览更新".into());
    }
    let merged = merged_overrides(&h, &p, &choices.unwrap_or_default())?;
    let mut store = h.store.clone();
    let i = store
        .instances
        .iter_mut()
        .find(|i| Some(&i.id) == store.active.as_ref())
        .unwrap();
    i.rollback_states
        .insert(i.package_hash.clone(), PackageState::capture(i));
    i.previous_packages.push(i.package_hash.clone());
    i.package_hash = hash;
    i.overrides = merged.0;
    i.profile = merged.1;
    i.profile_revision += 1;
    for r in i.revisions.values_mut() {
        *r += 1;
    }
    let id = i.id.clone();
    h.commit(store)?;
    h.cancel_generations();
    let now = h.now();
    h.scheduler.switch_instance(&id, now);
    h.package = Some(p);
    h.renderer_ready = false;
    h.previous_facts = (None, None);
    h.last_wall = 0;
    allow_assets(window.app_handle(), &h);
    Ok(json!(true))
}
#[tauri::command]
pub(super) fn restore_recommended(
    window: WebviewWindow,
    state: State<Shared>,
    owner: String,
) -> Result<(), String> {
    control(&window)?;
    let mut h = state.lock().unwrap();
    if !h.package.as_ref().is_some_and(|p| {
        p.manifest["pluginPresets"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|r| r["presetId"] == owner)
    }) {
        return Err("此项没有包推荐配置".into());
    }
    let mut store = h.store.clone();
    let i = store
        .instances
        .iter_mut()
        .find(|i| Some(&i.id) == store.active.as_ref())
        .ok_or("没有当前伙伴")?;
    if let Some(o) = i.overrides["plugins"].as_object_mut() {
        o.remove(&owner);
    }
    if let Some(a) = i.overrides["removedPresets"].as_array_mut() {
        a.retain(|v| v != &owner);
    }
    *i.revisions.entry(owner.clone()).or_default() += 1;
    h.commit(store)?;
    let now = h.now();
    h.scheduler.revoke_owner(&owner, now);
    h.last_wall = 0;
    Ok(())
}
