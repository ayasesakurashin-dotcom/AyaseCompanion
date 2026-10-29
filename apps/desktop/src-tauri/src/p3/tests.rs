use super::*;
#[test]
fn service_migration_defaults_never_enable_capture_or_new_network_services() {
    let old: services::Settings = serde_json::from_value(
        json!({"chat":{"enabled":false,"endpoint":"","model":""},"maxChars":8}),
    )
    .unwrap();
    assert!(!old.p3.screen_enabled);
    assert!(!old.p3.vision.enabled);
    assert!(!old.p3.search.enabled);
    assert!(!old.p3.weather_enabled);
    assert!(old.p3.chat_enabled);
    old.validate().unwrap();
}
#[test]
fn credentials_and_search_addresses_are_not_interchangeable() {
    for url in ["https://api.deepseek.com/v1", "https://api.deepseek.com/"] {
        assert!(search::official(url));
    }
    for url in [
        "http://api.deepseek.com/v1",
        "https://api.deepseek.com.evil.example/v1",
        "https://api.deepseek.com:444/v1",
        "https://api.deepseek.com/v1?key=secret",
        "https://user@api.deepseek.com/v1",
        "http://127.0.0.1:1234",
    ] {
        assert!(!search::official(url));
    }
}
#[test]
fn search_privacy_refuses_contacts_credentials_paths_and_private_hosts() {
    for q in [
        "someone@example.com",
        "sk-1234567890secret",
        &["C", ":", "\\private\\files"].concat(),
        "192.168.0.4",
        "token=hidden",
        "https://example.com/?key=secret",
    ] {
        assert!(!search::safe_query(q), "{q}");
    }
    assert!(search::safe_query("公开项目 2026 正式发布说明"));
    assert!(search::forbidden("请不要联网，解释这段文字"));
    assert!(!search::forbidden("翻译这段话：“不要联网”"));
    assert!(!search::public_url("file:///private/file"));
    assert!(!search::public_url("https://127.0.0.1/"));
    assert!(!search::public_url("https://host.local/"));
}
#[test]
fn observation_rejects_sensitive_unusable_truncated_and_oversized_material() {
    let capture = || screen::Capture {
        image: vec![],
        observed_at: "2026-10-29T12:00:00Z".into(),
        scope: "foreground".into(),
        label: "Synthetic App".into(),
    };
    let body = |v: Value| json!({"choices":[{"message":{"content":v.to_string()}}]});
    let good = json!({"usable":true,"sensitive":false,"confidence":0.8,"summary":"Synthetic evidence","visible_text":"FACT-123","uncertainties":["No evidence of actions performed"]});
    assert_eq!(
        parse_observation(&body(good.clone()), capture())
            .unwrap()
            .visible_text,
        "FACT-123"
    );
    for mut v in [good.clone(), good.clone(), good.clone()] {
        v["sensitive"] = json!(true);
        assert!(parse_observation(&body(v), capture()).is_err());
    }
    let mut huge = good;
    huge["summary"] = json!("x".repeat(501));
    assert!(parse_observation(&body(huge), capture()).is_err());
    assert!(
        parse_observation(&json!({"choices":[{"message":{"content":"{"}}]}), capture()).is_err()
    );
}
#[test]
fn observation_epoch_and_sources_remain_separate_from_user_memory() {
    let root = TestRoot::new();
    let m = crate::memory::Memory::open(&root.0).unwrap();
    let a = uuid::Uuid::new_v4().to_string();
    let b = uuid::Uuid::new_v4().to_string();
    let o = Observation {
        seq: 0,
        id: uuid::Uuid::new_v4().to_string(),
        observed_at: "2026-10-29T12:00:00Z".into(),
        scope: "foreground".into(),
        label: "Synthetic App".into(),
        summary: "Visible synthetic fact".into(),
        visible_text: "SCREEN-FACT".into(),
        uncertainties: vec![],
    };
    assert!(m.record_observation(&a, 0, &o).unwrap());
    assert!(m.observations(&b).unwrap().is_empty());
    assert_eq!(m.history(&a).unwrap()["pendingSync"], 0);
    m.user(&a, 0, "turn", "synthetic question", false).unwrap();
    m.presented_with_evidence(&a,0,"turn","shown answer","completed","chat",&json!({"observation":o,"search":{"status":"ok","sources":[{"url":"https://example.com/"}]}})).unwrap();
    assert_eq!(
        m.history(&a).unwrap()["messages"][1]["evidence"]["search"]["status"],
        "ok"
    );
    let context = m.context(&a, "next").unwrap();
    assert!(context.iter().any(|v| v["content"]
        .as_str()
        .is_some_and(|s| s.contains("历史外部资料回答"))));
    m.clear(&a, false).unwrap();
    assert!(m.observations(&a).unwrap().is_empty());
    assert!(!m.record_observation(&a, 0, &o).unwrap());
    assert!(!m
        .presented_with_evidence(
            &a,
            0,
            "late",
            "late",
            "completed",
            "chat",
            &json!({"sources":[1]})
        )
        .unwrap());
}
#[test]
fn special_dates_are_per_instance_and_invalid_dates_fail() {
    let root = TestRoot::new();
    let m = crate::memory::Memory::open(&root.0).unwrap();
    let a = uuid::Uuid::new_v4().to_string();
    let b = uuid::Uuid::new_v4().to_string();
    let mut date = calendar::SpecialDate {
        id: uuid::Uuid::new_v4().to_string(),
        name: "Synthetic anniversary".into(),
        month: 2,
        day: 29,
        ..Default::default()
    };
    date.validate().unwrap();
    assert!(!date.occurs(chrono::NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(), &[]));
    assert!(date.occurs(chrono::NaiveDate::from_ymd_opt(2028, 2, 29).unwrap(), &[]));
    m.save_special(&a, &[date.clone()]).unwrap();
    assert!(m.special_dates(&b).unwrap().is_empty());
    date.month = 4;
    date.day = 31;
    assert!(date.validate().is_err());
}
#[test]
fn base64_has_correct_padding() {
    assert_eq!(screen::base64(b"f"), "Zg==");
    assert_eq!(screen::base64(b"fo"), "Zm8=");
    assert_eq!(screen::base64(b"foo"), "Zm9v");
}
#[test]
fn observations_keep_originals_paginate_and_fence_late_summaries() {
    let root = TestRoot::new();
    let m = crate::memory::Memory::open(&root.0).unwrap();
    let a = uuid::Uuid::new_v4().to_string();
    let mut first = String::new();
    for n in 0..35 {
        let o = Observation {
            seq: 0,
            id: uuid::Uuid::new_v4().to_string(),
            observed_at: "2026-10-29T12:00:00Z".into(),
            scope: "foreground".into(),
            label: "Synthetic App".into(),
            summary: format!("Fact {n}"),
            visible_text: "Synthetic only".into(),
            uncertainties: vec![],
        };
        if n == 0 {
            first = o.id.clone();
        }
        m.record_observation(&a, 0, &o).unwrap();
    }
    let recent = m.observations(&a).unwrap();
    assert_eq!(recent.len(), 30);
    let older = m.observation_page(&a, recent.last().unwrap().seq).unwrap();
    assert_eq!(older.len(), 5);
    assert_eq!(m.observation_by_id(&a, &first).unwrap().summary, "Fact 0");
    let (epoch, revision, old, through, input) = m.observation_summary_input(&a).unwrap().unwrap();
    assert!(input.chars().count() < 8000);
    assert_eq!(m.observation_summary(&a).unwrap()["watermark"], 0);
    assert!(m
        .commit_observation_summary(
            &a,
            epoch,
            revision,
            old,
            through,
            "Synthetic source summary #1—16"
        )
        .unwrap());
    assert_eq!(m.observation_summary(&a).unwrap()["count"], 35);
    m.remove_observation(&a, &first).unwrap();
    assert!(!m
        .commit_observation_summary(&a, epoch, revision, through, 35, "late summary")
        .unwrap());
    assert_eq!(m.observation_summary(&a).unwrap()["summary"], "");
    m.clear(&a, false).unwrap();
    assert!(!m
        .commit_observation_summary(&a, epoch, revision, 0, 35, "late after forget")
        .unwrap());
}

struct TestRoot(std::path::PathBuf);
impl TestRoot {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("ayase-p3-test-{}", uuid::Uuid::new_v4())))
    }
}
impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
#[ignore = "explicit public provider integration; no credentials or personal location"]
fn real_public_environment_providers() {
    let settings = Settings {
        weather_enabled: true,
        city: "Tokyo public test point".into(),
        latitude: Some(35.68),
        longitude: Some(139.69),
        ..Default::default()
    };
    let w = weather(&settings).unwrap();
    assert_eq!(w["kind"], "model-estimate");
    assert!(w["temperature"].is_number());
    let cn = calendar::import("CN", 2026).unwrap();
    assert_eq!(cn.lunar.len(), 365);
    assert!(cn
        .events
        .iter()
        .any(|e| e.name == "春节" && e.date.to_string() == "2026-02-17"));
    let jp = calendar::import("JP", 2026).unwrap();
    assert!(!jp.events.is_empty());
}
