use super::*;
fn fixture() -> (super::super::tests::TestRoot, Host) {
    let root = super::super::tests::TestRoot::new();
    let mut h = Host::open(root.0.clone()).unwrap();
    let p = h
        .stage_archive(&super::super::tests::archive("0.2.0"))
        .unwrap();
    activate(&mut h, &p.hash, "new").unwrap();
    h.facts.lock().unwrap().idle_seconds = Some(0);
    (root, h)
}
fn add(h: &mut Host, owner: &str, priority: &str) {
    h.instance_mut().unwrap().overrides["extraPresets"].as_array_mut().unwrap().push(json!({"presetId":owner,"pluginId":"ayase.routine-care","configVersion":"1.0","enabled":true,"priority":priority,"config":{"start":"00:00","end":"00:00","response":{"mode":"fixed","variants":[{"id":"fixed","text":"synthetic care"}]}}}));
}
#[test]
fn shared_budget_accepts_one_higher_priority_care_and_preserves_current_reply() {
    let (_root, mut h) = fixture();
    add(&mut h, "background-care", "background");
    add(&mut h, "important-care", "important");
    let id = h
        .present(
            json!({"id":"user","text":"current reply"}),
            "host.user",
            Priority::User,
            false,
            60_000,
            0,
        )
        .unwrap();
    h.scheduler.tick(h.now());
    let generation = h
        .scheduler
        .tasks
        .iter()
        .find(|t| t.id == id)
        .unwrap()
        .generation;
    h.scheduler.started(id, generation, h.now());
    h.p3_tick(Utc::now(), None, None);
    assert_eq!(h.scheduler.tasks.len(), 1);
    assert_eq!(h.scheduler.active().unwrap().id, id);
    h.scheduler.finish(
        id,
        ayase_core::scheduler::Status::Completed,
        "test",
        h.now(),
    );
    h.p3_tick(Utc::now(), None, None);
    assert_eq!(h.scheduler.tasks.len(), 2);
    assert_eq!(h.scheduler.tasks.back().unwrap().owner, "important-care");
    h.scheduler.finish(
        h.scheduler.tasks.back().unwrap().id,
        ayase_core::scheduler::Status::Completed,
        "test",
        h.now(),
    );
    h.p3_tick(Utc::now(), None, None);
    assert_eq!(h.scheduler.tasks.len(), 2);
}
#[test]
fn budget_survives_restart_and_preview_data_never_crosses_instances() {
    let (root, mut h) = fixture();
    add(&mut h, "care", "normal");
    h.p3_tick(Utc::now(), None, None);
    assert_eq!(h.scheduler.tasks.len(), 1);
    let old = h.instance().unwrap().id.clone();
    h.p3.info = json!({"observations":[{"summary":"A-only"}]});
    assert_eq!(
        h.p3_snapshot()["info"]["observations"][0]["summary"],
        "A-only"
    );
    let hash = h.instance().unwrap().package_hash.clone();
    activate(&mut h, &hash, "new").unwrap();
    assert!(h.p3_snapshot()["info"].is_null());
    let mut store = h.store.clone();
    store.active = Some(old.clone());
    h.commit(store).unwrap();
    drop(h);
    let mut h = Host::open(root.0.clone()).unwrap();
    assert!(
        h.instance().unwrap().runtime.state["p3:budget"]["count"]
            .as_u64()
            .unwrap()
            > 0
    );
    h.p3_tick(Utc::now(), None, None);
    assert!(h.scheduler.tasks.is_empty());
}
