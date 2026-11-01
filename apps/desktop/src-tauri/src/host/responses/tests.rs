use super::super::tests::{archive, TestRoot};
use super::*;

fn fixture() -> (TestRoot, Host) {
    let root = TestRoot::new();
    let mut h = Host::open(root.0.clone()).unwrap();
    let p = h.stage_archive(&archive("0.3.0")).unwrap();
    activate(&mut h, &p.hash, "new").unwrap();
    let m = h.package.as_mut().unwrap().model.as_mut().unwrap();
    m["baseModes"]["music"] = json!({"loop":"idle"});
    m["baseModes"]["quiet"] = json!({"loop":"idle"});
    (root, h)
}
fn fixed(text: &str) -> Value {
    json!({"mode":"fixed","variants":[{"id":text,"text":text}]})
}
fn routes() -> Value {
    json!({"mode":"state-specific","stateResponses":[{"id":"idle-route","base":"idle","response":{"mode":"fixed","variants":[{"id":"a","text":"A"},{"id":"b","text":"B"}]}},{"id":"music-route","base":"music","response":{"mode":"fixed","variants":[{"id":"c","text":"C"},{"id":"d","text":"D"}]}}]})
}
fn begin(h: &mut Host) -> (u64, u64) {
    h.scheduler.tick(h.now());
    h.prepare_variant_start();
    let t = h.scheduler.active().unwrap().clone();
    assert!(h.start_presentation(t.id, t.generation, h.now()));
    (t.id, t.generation)
}

#[test]
fn specific_route_precedes_general_and_missing_state_never_uses_old_body() {
    let (_root, mut h) = fixture();
    let mut response = routes();
    response["intent"] = json!("must not use stale top-level intent");
    h.scheduler.claim("media", "music", Priority::Normal, false);
    // Reconcile has not run: logical state still reflects the new effective claim.
    assert_eq!(h.scheduler.desired_base, "idle");
    assert_eq!(h.response_base(), "music");
    let (_, guard) = h.select_response(&response, "plugin").unwrap();
    assert_eq!(guard["routeId"], "music-route");
    h.scheduler
        .claim("fullscreen", "quiet", Priority::Important, true);
    assert!(h.select_response(&response, "plugin").is_err());
    assert!(h.scheduler.tasks.is_empty());
    response["stateResponses"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"all","base":null,"response":fixed("general")}));
    let (selected, guard) = h.select_response(&response, "plugin").unwrap();
    assert_eq!(selected, fixed("general"));
    assert_eq!(guard["base"], "quiet");
    h.scheduler.claims.remove("fullscreen");
    let (_, guard) = h.select_response(&response, "plugin").unwrap();
    assert_eq!(guard["routeId"], "music-route");
    assert_eq!(
        h.select_response(&fixed("legacy"), "plugin").unwrap().1,
        Value::Null
    );
}

#[test]
fn changed_state_discards_queued_or_leased_reply_without_touching_started_task() {
    let (_root, mut h) = fixture();
    let first = h
        .present_variants(routes(), "plugin", Priority::Normal, false, 60000, 0)
        .unwrap();
    let (id, generation) = begin(&mut h);
    assert_eq!(id, first);
    let queued = h
        .present_variants(routes(), "plugin", Priority::Important, false, 60000, 0)
        .unwrap();
    h.scheduler.claim("media", "music", Priority::Normal, false);
    h.discard_stale_responses();
    assert_eq!(h.scheduler.active().unwrap().id, first);
    assert_eq!(h.scheduler.active().unwrap().status, Status::Presenting);
    assert_eq!(
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == queued)
            .unwrap()
            .status,
        Status::Cancelled
    );
    assert!(h.response_state_valid(first));
    assert!(h
        .scheduler
        .channel(first, generation, "text", false, h.now()));
    let leased = h
        .present_variants(routes(), "plugin", Priority::Normal, false, 60000, 0)
        .unwrap();
    h.scheduler.tick(h.now());
    h.prepare_variant_start();
    let generation = h.scheduler.active().unwrap().generation;
    h.scheduler.claims.remove("media");
    assert!(!h.start_presentation(leased, generation, h.now()));
    assert_eq!(
        h.scheduler
            .tasks
            .iter()
            .find(|t| t.id == leased)
            .unwrap()
            .status,
        Status::Cancelled
    );
    assert!(!h
        .instance()
        .unwrap()
        .runtime
        .state
        .contains_key("variant:plugin:fixed:state:music-route"));
}

#[test]
fn state_routes_keep_independent_no_repeat_history() {
    let (_root, mut h) = fixture();
    let mut last_idle = Value::Null;
    let mut last_music = Value::Null;
    for _ in 0..3 {
        for (base, last) in [("idle", &mut last_idle), ("music", &mut last_music)] {
            h.scheduler.claims.clear();
            if base == "music" {
                h.scheduler.claim("media", "music", Priority::Normal, false);
            }
            h.present_variants(routes(), "plugin", Priority::Normal, false, 60000, 0)
                .unwrap();
            let (id, g) = begin(&mut h);
            let selected = h.scheduler.active().unwrap().plan["fixedChoice"]["id"].clone();
            assert_ne!(selected, *last);
            *last = selected;
            assert!(h.scheduler.channel(id, g, "text", false, h.now()));
        }
    }
}

#[test]
fn direct_animation_ids_and_asset_aliases_preserve_order_and_task_channels() {
    let (_root, mut h) = fixture();
    let m = h.package.as_mut().unwrap().model.as_mut().unwrap();
    m["clips"]["anime.music_start"] = m["clips"]["idle"].clone();
    m["clips"]["anime.music_start"]["asset"] = json!("music-atlas");
    m["semanticCues"] = json!({"greeting":{"clip":"idle","repeat":2}});
    let (plan,channels)=h.resolve_variant(&json!({"id":"direct","text":"short","cue":"greeting","animationAsset":"anime.music_start,clip:idle\nmusic-atlas"}),"plugin").unwrap();
    assert_eq!(
        plan["clips"],
        json!([
            "idle",
            "idle",
            "anime.music_start",
            "idle",
            "anime.music_start"
        ])
    );
    assert_eq!(channels, vec!["text", "animation"]);
    assert_eq!(h.response_base(), "idle");
    h.package.as_mut().unwrap().model.as_mut().unwrap()["clips"]["other"] =
        h.package.as_ref().unwrap().model.as_ref().unwrap()["clips"]["anime.music_start"].clone();
    assert!(h
        .resolve_variant(&json!({"animationAsset":"music-atlas"}), "plugin")
        .is_err());
    let (plan, _) = h
        .resolve_variant(&json!({"animationAsset":"anime.music_start"}), "plugin")
        .unwrap();
    assert_eq!(plan["clips"], json!(["anime.music_start"]));
}
