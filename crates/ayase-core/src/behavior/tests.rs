use super::*;
fn config(kind: &str) -> Configured {
    let p = crate::plugin::builtin()
        .remove(&format!("ayase.{kind}"))
        .unwrap();
    p.configure(&json!({"presetId":"test","configVersion":"1.0","enabled":true,"config":{"start":"00:00","end":"00:00","timeZone":"UTC"}}),&json!({}),0)
}
fn wall() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-29T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
#[test]
fn work_counter_distinguishes_manual_pause_idle_rest_and_suspend() {
    let mut c = config("work-reminder");
    c.config["thresholdSeconds"] = json!(3);
    c.config["basis"] = json!("manual");
    let mut state = BTreeMap::new();
    let mut input = Input {
        manual_work: true,
        idle_seconds: Some(0),
        ..Default::default()
    };
    assert!(evaluate("work-reminder", &c, &input, wall(), 0., &mut state).is_none());
    input.monotonic_ms = 1000;
    assert!(evaluate("work-reminder", &c, &input, wall(), 0., &mut state).is_none());
    input.manual_work = false;
    input.monotonic_ms = 2000;
    assert!(evaluate("work-reminder", &c, &input, wall(), 0., &mut state).is_none());
    input.manual_work = true;
    input.monotonic_ms = 3000;
    assert!(evaluate("work-reminder", &c, &input, wall(), 0., &mut state).is_none());
    input.monotonic_ms = 4000;
    let due = evaluate("work-reminder", &c, &input, wall(), 0., &mut state).unwrap();
    assert_eq!(due.facts["basis"], "manual");
    input.manual_rest = true;
    input.monotonic_ms = 5000;
    assert!(evaluate("work-reminder", &c, &input, wall(), 0., &mut state).is_none());
    input.manual_rest = false;
    c.config["thresholdSeconds"] = json!(10);
    input.monotonic_ms = 600_000;
    assert!(evaluate("work-reminder", &c, &input, wall(), 0., &mut state).is_none());
    assert_eq!(state["p3:test:0"]["work"], 3000);
}
#[test]
fn random_intervals_wait_for_acceptance_cooldown_and_daily_limit() {
    let mut c = config("random-interaction");
    c.config["minSeconds"] = json!(2);
    c.config["maxSeconds"] = json!(4);
    c.config["cooldownSeconds"] = json!(1);
    c.config["dailyLimit"] = json!(1);
    let mut input = Input::default();
    let mut state = BTreeMap::new();
    assert!(evaluate("random-interaction", &c, &input, wall(), 0.5, &mut state).is_none());
    input.monotonic_ms = 2999;
    assert!(evaluate("random-interaction", &c, &input, wall(), 0.5, &mut state).is_none());
    input.monotonic_ms = 3000;
    let due = evaluate("random-interaction", &c, &input, wall(), 0.5, &mut state).unwrap();
    assert!(evaluate("random-interaction", &c, &input, wall(), 0.5, &mut state).is_some());
    accepted(&c, "random-interaction", &due.key, 3000, 0.5, &mut state);
    input.monotonic_ms = 20_000;
    assert!(evaluate("random-interaction", &c, &input, wall(), 0.5, &mut state).is_none());
}
#[test]
fn missing_weather_and_unknown_activity_never_create_facts() {
    let c = config("weather-care");
    assert!(evaluate(
        "weather-care",
        &c,
        &Input::default(),
        wall(),
        0.,
        &mut BTreeMap::new()
    )
    .is_none());
    let c = config("routine-care");
    assert!(evaluate(
        "routine-care",
        &c,
        &Input::default(),
        wall(),
        0.,
        &mut BTreeMap::new()
    )
    .is_none());
}
#[test]
fn repeated_weather_refresh_has_one_semantic_key() {
    let c = config("weather-care");
    let mut input = Input {
        weather: Some(json!({"city":"Synthetic City","temperature":0.,"code":0,"fetchedAt":"a"})),
        ..Default::default()
    };
    let mut state = BTreeMap::new();
    let due = evaluate("weather-care", &c, &input, wall(), 0., &mut state).unwrap();
    accepted(&c, "weather-care", &due.key, 0, 0., &mut state);
    input.monotonic_ms = 3_000_000;
    input.weather =
        Some(json!({"city":"Synthetic City","temperature":1.,"code":0,"fetchedAt":"b"}));
    assert!(evaluate("weather-care", &c, &input, wall(), 0., &mut state).is_none());
}
#[test]
fn pointer_modes_are_explicit_and_not_proactive() {
    let c = config("pointer-interaction");
    let mut input = Input {
        pointer: Some(("single".into(), String::new())),
        ..Default::default()
    };
    let mut state = BTreeMap::new();
    assert!(evaluate("pointer-interaction", &c, &input, wall(), 0., &mut state).is_none());
    input.pointer = Some(("double".into(), String::new()));
    assert!(
        !evaluate("pointer-interaction", &c, &input, wall(), 0., &mut state)
            .unwrap()
            .proactive
    );
}
#[test]
fn windows_include_start_exclude_end_and_cross_midnight() {
    assert!(in_window(23 * 60, "23:00", "07:00"));
    assert!(in_window(6 * 60 + 59, "23:00", "07:00"));
    assert!(!in_window(7 * 60, "23:00", "07:00"));
    assert!(!in_window(22 * 60, "23:00", "07:00"));
    assert!(!in_window(0, "bad", "00:00"));
}
#[test]
fn daily_and_interval_reminders_use_distinct_occurrences_and_no_resume_burst() {
    let mut c = config("scheduled-reminder");
    c.config["time"] = json!("12:00");
    c.config["cooldownSeconds"] = json!(1);
    let mut input = Input::default();
    let mut state = BTreeMap::new();
    let due = evaluate("scheduled-reminder", &c, &input, wall(), 0., &mut state).unwrap();
    accepted(&c, "scheduled-reminder", &due.key, 0, 0., &mut state);
    input.monotonic_ms = 5000;
    assert!(evaluate("scheduled-reminder", &c, &input, wall(), 0., &mut state).is_none());
    c.config["repeat"] = json!("interval");
    c.config["intervalSeconds"] = json!(3);
    state.clear();
    input.monotonic_ms = 0;
    assert!(evaluate("scheduled-reminder", &c, &input, wall(), 0., &mut state).is_none());
    input.monotonic_ms = 3000;
    assert!(evaluate("scheduled-reminder", &c, &input, wall(), 0., &mut state).is_some());
}
