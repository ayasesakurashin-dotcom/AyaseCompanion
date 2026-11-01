use super::*;
use ayase_core::scheduler::Status;

impl Host {
    pub(super) fn response_base(&self) -> String {
        self.package
            .as_ref()
            .and_then(|p| p.model.as_ref())
            .map(|m| self.scheduler.effective_base(m))
            .unwrap_or_else(|| "idle".into())
    }
    pub(super) fn select_response(
        &mut self,
        response: &Value,
        owner: &str,
    ) -> Result<(Value, Value), String> {
        plugin::validate_response(response)?;
        let base = self.response_base();
        let Some((selected, route)) = plugin::response_for_base(response, &base) else {
            self.scheduler.record(
                self.now(),
                "response-state-skipped",
                json!({"owner":owner,"base":base,"reason":"没有状态专用或通用回复"}),
            );
            return Err(format!("当前{base}没有匹配回复，已跳过"));
        };
        let guard = route
            .map(|id| json!({"base":base,"routeId":id}))
            .unwrap_or(Value::Null);
        Ok((selected.clone(), guard))
    }
    pub(super) fn response_state_valid(&mut self, id: u64) -> bool {
        let Some(task) = self
            .scheduler
            .tasks
            .iter()
            .find(|t| {
                t.id == id
                    && !t.status.terminal()
                    && t.status != Status::Presenting
                    && t.plan["stateGuard"].is_object()
            })
            .cloned()
        else {
            return true;
        };
        let base = self.response_base();
        if task.plan["stateGuard"]["base"] == base {
            return true;
        }
        let now = self.now();
        self.scheduler.record(now,"response-state-skipped",json!({"taskId":id,"owner":task.owner,"expected":task.plan["stateGuard"]["base"],"base":base,"routeId":task.plan["stateGuard"]["routeId"],"reason":"开始前基础状态已改变"}));
        self.scheduler.finish(
            id,
            Status::Cancelled,
            "开始前基础状态已改变，跳过旧回复",
            now,
        );
        false
    }
    pub(super) fn discard_stale_responses(&mut self) {
        let ids: Vec<_> = self
            .scheduler
            .tasks
            .iter()
            .filter(|t| {
                !t.status.terminal()
                    && t.status != Status::Presenting
                    && t.plan["stateGuard"].is_object()
            })
            .map(|t| t.id)
            .collect();
        for id in ids {
            self.response_state_valid(id);
        }
    }
}

#[cfg(test)]
mod tests;
