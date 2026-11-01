use super::*;
use ayase_core::scheduler::Status;
use sha2::{Digest, Sha256};

fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap()))
}

impl Host {
    pub(super) fn variant_plan(
        &self,
        response: &Value,
        owner: &str,
        kind: &str,
        unit: f64,
    ) -> Result<(Value, Vec<String>), String> {
        self.variant_plan_guarded(response, owner, kind, &Value::Null, unit)
    }
    pub(super) fn variant_plan_guarded(
        &self,
        response: &Value,
        owner: &str,
        kind: &str,
        guard: &Value,
        unit: f64,
    ) -> Result<(Value, Vec<String>), String> {
        plugin::validate_response(response)?;
        let scope = digest(&response["variants"]);
        let key = match guard["routeId"].as_str() {
            Some(route) => format!("variant:{owner}:{kind}:state:{route}"),
            None => format!("variant:{owner}:{kind}"),
        };
        let previous = self
            .instance()
            .and_then(|i| i.runtime.state.get(&key))
            .filter(|v| v["scope"] == scope);
        let mut candidates = vec![];
        for variant in response["variants"].as_array().ok_or("缺少回复条目")? {
            if let Ok((plan, channels)) = self.resolve_variant(variant, owner) {
                if !channels.is_empty() {
                    let effect = digest(
                        &json!({"text":plan["textSegments"],"audio":plan["audioSegments"],"clips":plan["clips"]}),
                    );
                    candidates.push((variant.clone(), plan, channels, effect));
                }
            }
        }
        let different = candidates
            .iter()
            .any(|(_, _, _, effect)| previous.is_none_or(|p| p["effect"] != *effect));
        if different {
            candidates.retain(|(_, _, _, effect)| previous.is_none_or(|p| p["effect"] != *effect));
        }
        let selected = plugin::choose_variant(
            &json!({"variants":candidates.iter().map(|c| c.0.clone()).collect::<Vec<_>>()}),
            unit,
        )
        .ok_or("没有可呈现的固定素材")?;
        let (_, mut plan, channels, effect) = candidates
            .into_iter()
            .find(|c| c.0["id"] == selected["id"])
            .unwrap();
        plan["fixedChoice"] = json!({"response":response,"kind":kind,"key":key,"scope":scope,"id":selected["id"],"effect":effect,"prepared":false});
        if guard.is_object() {
            plan["stateGuard"] = guard.clone();
        }
        Ok((plan, channels))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn present_variants(
        &mut self,
        response: Value,
        owner: &str,
        priority: Priority,
        proactive: bool,
        ttl: u64,
        revision: u64,
    ) -> Result<u64, String> {
        let (response, guard) = self.select_response(&response, owner)?;
        self.present_selected_variants(response, guard, owner, priority, proactive, ttl, revision)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn present_selected_variants(
        &mut self,
        response: Value,
        guard: Value,
        owner: &str,
        priority: Priority,
        proactive: bool,
        ttl: u64,
        revision: u64,
    ) -> Result<u64, String> {
        let (plan, channels) = if guard.is_object() {
            self.variant_plan_guarded(&response, owner, "fixed", &guard, random_unit())?
        } else {
            self.variant_plan(&response, owner, "fixed", random_unit())?
        };
        let instance_id = self.instance().ok_or("没有活动伙伴")?.id.clone();
        self.scheduler.submit(
            Request {
                instance_id,
                owner: owner.into(),
                revision,
                priority,
                proactive,
                ttl_ms: ttl,
                preparing: false,
                plan,
                channels,
                dedupe: None,
            },
            self.now(),
        )
    }

    /// Resolve once at the presentation lease, after queue ordering and previous
    /// actual starts are known. Expired/cancelled queued choices never enter history.
    pub(super) fn prepare_variant_start(&mut self) {
        let Some(task) = self
            .scheduler
            .active()
            .filter(|t| {
                t.status == Status::Starting
                    && t.plan["fixedChoice"].is_object()
                    && t.plan["fixedChoice"]["prepared"] != true
            })
            .cloned()
        else {
            return;
        };
        let choice = &task.plan["fixedChoice"];
        if !self.response_state_valid(task.id) {
            return;
        }
        match self.variant_plan_guarded(
            &choice["response"],
            &task.owner,
            choice["kind"].as_str().unwrap_or("fixed"),
            &task.plan["stateGuard"],
            random_unit(),
        ) {
            Ok((mut plan, channels)) => {
                if !task.plan["evidence"].is_null() {
                    plan["evidence"] = task.plan["evidence"].clone();
                }
                plan["fixedChoice"]["prepared"] = json!(true);
                let t = self
                    .scheduler
                    .tasks
                    .iter_mut()
                    .find(|t| t.id == task.id)
                    .unwrap();
                t.plan = plan;
                t.channels = channels
                    .into_iter()
                    .map(|c| (c, "pending".into()))
                    .collect();
            }
            Err(reason) => self
                .scheduler
                .finish(task.id, Status::Failed, &reason, self.now()),
        }
    }

    pub(super) fn start_presentation(&mut self, id: u64, generation: u64, now: u64) -> bool {
        let choice = self
            .scheduler
            .active()
            .filter(|t| t.id == id && t.generation == generation && t.status == Status::Starting)
            .map(|t| t.plan["fixedChoice"].clone());
        if choice
            .as_ref()
            .is_some_and(|c| c.is_object() && c["prepared"] != true)
        {
            return false;
        }
        if !self.response_state_valid(id)
            || !self.generation_start_valid(id)
            || !self.scheduler.started(id, generation, now)
        {
            return false;
        }
        if let Some(choice) = choice.filter(Value::is_object) {
            let key = choice["key"].as_str().unwrap().to_owned();
            if let Some(i) = self.instance_mut() {
                i.runtime.state.insert(
                    key,
                    json!({"scope":choice["scope"],"effect":choice["effect"],"id":choice["id"]}),
                );
            }
            self.scheduler.record(
                now,
                "variant-started",
                json!({"taskId":id,"variantId":choice["id"],"kind":choice["kind"]}),
            );
            if let Err(reason) = self.commit(self.store.clone()) {
                self.scheduler.record(now, "save-failed", json!(reason));
            }
        }
        true
    }
}

pub(super) fn random_unit() -> f64 {
    (Uuid::new_v4().as_u128() >> 64) as f64 / u64::MAX as f64
}

#[cfg(test)]
mod tests;
