//! Explicit wire dialects; unknown compatible models receive no guessed extensions.
use super::Provider;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Thinking {
    pub protocol: String,
    pub effort: String,
}
impl Default for Thinking {
    fn default() -> Self {
        Self {
            protocol: "auto".into(),
            effort: "off".into(),
        }
    }
}
impl Thinking {
    pub fn validate(&self) -> Result<(), String> {
        if !["auto", "none", "deepseek", "openai", "qwen"].contains(&self.protocol.as_str())
            || !["off", "low", "medium", "high", "max"].contains(&self.effort.as_str())
        {
            return Err("思考接口或强度选项无效".into());
        }
        Ok(())
    }
    pub fn disabled(&self) -> Self {
        Self {
            effort: "off".into(),
            ..self.clone()
        }
    }
    pub fn dialect(&self, p: &Provider) -> &str {
        if self.protocol != "auto" {
            return &self.protocol;
        }
        let m = p.model.to_ascii_lowercase();
        if m.starts_with("deepseek-flash")
            || m.starts_with("deepseek-pro")
            || m.starts_with("deepseek-v4")
        {
            "deepseek"
        } else if m.starts_with("qwen3") && !m.contains("thinking") {
            "qwen"
        } else if m.starts_with("gpt-5.")
            || m.starts_with("gpt-6")
            || m.starts_with("o3")
            || m.starts_with("o4")
        {
            "openai"
        } else {
            "none"
        }
    }
    pub fn apply(&self, p: &Provider, body: &mut Value) -> Result<(), String> {
        self.validate()?;
        let on = self.effort != "off";
        if on && self.dialect(p) != "none" {
            if let Some(n) = body["max_tokens"].as_u64() {
                body["max_tokens"] = json!(
                    n + match self.effort.as_str() {
                        "low" => 1024,
                        "medium" => 4096,
                        "high" => 8192,
                        _ => 16384,
                    }
                );
            }
        }
        match self.dialect(p) {
            "deepseek" => {
                body["thinking"] = json!({"type":if on{"enabled"}else{"disabled"}});
                if on {
                    body["reasoning_effort"] = json!(match self.effort.as_str() {
                        "low" => "low",
                        "max" => "max",
                        _ => "high",
                    });
                }
            }
            "qwen" => {
                if !on && p.model.to_ascii_lowercase().contains("thinking") {
                    return Err("此型号为固定思考模型，请选择可关闭思考的型号或启用思考".into());
                }
                body["enable_thinking"] = json!(on);
                if on {
                    body["thinking_budget"] = json!(match self.effort.as_str() {
                        "low" => 1024,
                        "medium" => 4096,
                        "high" => 8192,
                        _ => 16384,
                    });
                }
            }
            "openai" => {
                let m = p.model.to_ascii_lowercase();
                if !on
                    && (m.starts_with("gpt-6")
                        || m.starts_with("o3")
                        || m.starts_with("o4")
                        || m == "gpt-5"
                        || m.starts_with("gpt-5-mini")
                        || m.starts_with("gpt-5-nano"))
                {
                    return Err(
                        "此型号不支持关闭思考，请选择低/中/高强度或使用可关闭思考的型号".into(),
                    );
                }
                if self.effort == "max" && !m.starts_with("gpt-6") {
                    return Err("当前型号未声明支持最高思考强度，请选择高强度或核对服务文档".into());
                }
                body["reasoning_effort"] = json!(if on { self.effort.as_str() } else { "none" });
                if let Some(tokens) = body.as_object_mut().unwrap().remove("max_tokens") {
                    body["max_completion_tokens"] = tokens;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_disable_known_hybrids_and_leave_unknown_alone() {
        for (model, key, value) in [
            ("deepseek-flash", "thinking", json!({"type":"disabled"})),
            ("qwen3-plus", "enable_thinking", json!(false)),
            ("gpt-5.5", "reasoning_effort", json!("none")),
        ] {
            let p = Provider {
                model: model.into(),
                ..Default::default()
            };
            let mut body = json!({"max_tokens":100});
            Thinking::default().apply(&p, &mut body).unwrap();
            assert_eq!(body[key], value);
        }
        let p = Provider {
            model: "unknown-compatible".into(),
            ..Default::default()
        };
        let mut body = json!({"max_tokens":100});
        Thinking::default().apply(&p, &mut body).unwrap();
        assert_eq!(body, json!({"max_tokens":100}));
    }
    #[test]
    fn explicit_dialects_map_effort_and_reject_impossible_off() {
        let p = Provider {
            model: "deepseek-flash".into(),
            ..Default::default()
        };
        let mut body = json!({});
        Thinking {
            protocol: "deepseek".into(),
            effort: "medium".into(),
        }
        .apply(&p, &mut body)
        .unwrap();
        assert_eq!(body["reasoning_effort"], "high");
        assert_eq!(body["thinking"]["type"], "enabled");
        let p = Provider {
            model: "gpt-6.1-sol".into(),
            ..Default::default()
        };
        assert!(Thinking::default().apply(&p, &mut json!({})).is_err());
    }
}
