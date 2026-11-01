use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Background,
    Normal,
    Important,
    User,
}

impl Priority {
    pub fn plugin(v: &str) -> Result<Self, String> {
        match v {
            "background" => Ok(Self::Background),
            "normal" => Ok(Self::Normal),
            "important" => Ok(Self::Important),
            _ => Err("插件优先级只允许 background / normal / important".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Preparing,
    Ready,
    Starting,
    Presenting,
    Completed,
    Cancelled,
    Expired,
    Failed,
}
impl Status {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Expired | Self::Failed
        )
    }
    pub fn pending(self) -> bool {
        matches!(self, Self::Queued | Self::Preparing | Self::Ready)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: u64,
    pub instance_id: String,
    pub owner: String,
    pub revision: u64,
    pub sequence: u64,
    pub generation: u64,
    pub priority: Priority,
    pub status: Status,
    pub accepted_at: u64,
    pub deadline: u64,
    pub prepare_deadline: u64,
    pub lease_deadline: u64,
    pub proactive: bool,
    pub plan: Value,
    pub channels: BTreeMap<String, String>,
    pub target_base: Option<String>,
    pub result: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub instance_id: String,
    pub owner: String,
    pub revision: u64,
    pub priority: Priority,
    pub proactive: bool,
    pub ttl_ms: u64,
    pub preparing: bool,
    pub plan: Value,
    pub channels: Vec<String>,
    pub dedupe: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Claim {
    pub owner: String,
    pub mode: String,
    pub priority: Priority,
    pub sequence: u64,
    pub suppress: bool,
}

/// A base-only closing clip may use the free animation lane after the foreground
/// action ended. It never owns or restarts that reply's text/audio channels.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseTransition {
    pub id: u64,
    pub generation: u64,
    pub instance_id: String,
    pub parent_task_id: u64,
    pub owner: String,
    pub from_base: String,
    pub target_base: String,
    pub clips: Vec<String>,
    pub status: Status,
    pub lease_deadline: u64,
}

#[derive(Debug, Default)]
pub struct Scheduler {
    pub tasks: VecDeque<Task>,
    pub claims: BTreeMap<String, Claim>,
    pub desired_base: String,
    pub applied_base: String,
    pub base_transition: Option<BaseTransition>,
    pub log: VecDeque<Value>,
    sequence: u64,
    generation: u64,
    pub current_instance: String,
    pub revisions: BTreeMap<String, u64>,
    dedupe: BTreeMap<String, u64>,
    pub muted: bool,
}

impl Scheduler {
    pub fn new(instance: &str) -> Self {
        Self {
            current_instance: instance.into(),
            desired_base: "idle".into(),
            applied_base: "idle".into(),
            ..Default::default()
        }
    }
    fn seq(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }
    pub fn record(&mut self, now: u64, event: &str, detail: Value) {
        self.log
            .push_back(json!({"atMs":now,"event":event,"detail":detail}));
        while self.log.len() > 240 {
            self.log.pop_front();
        }
    }
    pub fn submit(&mut self, r: Request, now: u64) -> Result<u64, String> {
        if r.instance_id != self.current_instance {
            return Err("角色实例已变化".into());
        }
        if r.owner != "host.user" && r.priority == Priority::User {
            return Err("插件不能使用 user 优先级".into());
        }
        if r.channels
            .iter()
            .any(|x| !["text", "audio", "animation"].contains(&x.as_str()))
        {
            return Err("未知呈现通道".into());
        }
        if self.tasks.iter().filter(|t| !t.status.terminal()).count() >= 64 {
            return Err("候选队列已满（64）".into());
        }
        if r.channels.is_empty() && !r.preparing {
            self.record(now, "skipped", json!({"owner":r.owner}));
            return Err("无可呈现通道，已跳过".into());
        }
        self.dedupe.retain(|_, until| *until > now);
        if let Some(key) = &r.dedupe {
            let key = format!("{}:{}:{}", r.instance_id, r.owner, key);
            if self.dedupe.contains_key(&key) {
                return Err("重复候选已跳过".into());
            }
            self.dedupe.insert(key, now + r.ttl_ms.clamp(1, 3_600_000));
        }
        let id = self.seq();
        self.generation += 1;
        let task = Task {
            id,
            instance_id: r.instance_id,
            owner: r.owner,
            revision: r.revision,
            sequence: id,
            generation: self.generation,
            priority: r.priority,
            status: if r.preparing {
                Status::Preparing
            } else {
                Status::Ready
            },
            accepted_at: now,
            deadline: now + r.ttl_ms.clamp(1, 3_600_000),
            prepare_deadline: now + 30_000,
            lease_deadline: 0,
            proactive: r.proactive,
            plan: r.plan,
            channels: r
                .channels
                .into_iter()
                .map(|c| (c, "pending".into()))
                .collect(),
            target_base: None,
            result: None,
        };
        self.record(
            now,
            "accepted",
            json!({"taskId":id,"owner":task.owner,"priority":task.priority}),
        );
        self.tasks.push_back(task);
        self.trim();
        Ok(id)
    }
    fn trim(&mut self) {
        while self.tasks.len() > 160 {
            if let Some(i) = self.tasks.iter().position(|t| t.status.terminal()) {
                self.tasks.remove(i);
            } else {
                break;
            }
        }
    }
    pub fn active(&self) -> Option<&Task> {
        self.tasks
            .iter()
            .find(|t| matches!(t.status, Status::Starting | Status::Presenting))
    }
    pub fn valid(&self, id: u64, generation: u64) -> bool {
        self.active().is_some_and(|t| {
            t.id == id && t.generation == generation && t.instance_id == self.current_instance
        })
    }
    pub fn ready(
        &mut self,
        id: u64,
        generation: u64,
        plan: Value,
        channels: Vec<String>,
        now: u64,
    ) -> bool {
        let Some(t) = self
            .tasks
            .iter_mut()
            .find(|t| t.id == id && t.generation == generation && t.status == Status::Preparing)
        else {
            return false;
        };
        if now >= t.deadline || now >= t.prepare_deadline {
            return false;
        }
        t.plan = plan;
        t.channels = channels
            .into_iter()
            .map(|c| (c, "pending".into()))
            .collect();
        t.status = if t.channels.is_empty() {
            Status::Completed
        } else {
            Status::Ready
        };
        true
    }
    pub fn finish(&mut self, id: u64, status: Status, reason: &str, now: u64) {
        let Some(t) = self
            .tasks
            .iter_mut()
            .find(|t| t.id == id && !t.status.terminal())
        else {
            return;
        };
        t.status = status;
        t.result = Some(reason.into());
        if status == Status::Completed {
            if let Some(target) = &t.target_base {
                self.applied_base = target.clone();
            }
        }
        self.record(
            now,
            "finished",
            json!({"taskId":id,"status":status,"reason":reason}),
        );
        if status != Status::Completed
            && self
                .base_transition
                .as_ref()
                .is_some_and(|t| t.parent_task_id == id)
        {
            self.end_base_transition("cancelled", now);
        }
    }
    pub fn started(&mut self, id: u64, generation: u64, now: u64) -> bool {
        let Some(t) = self.tasks.iter_mut().find(|t| {
            t.id == id
                && t.generation == generation
                && matches!(t.status, Status::Starting | Status::Presenting)
        }) else {
            return false;
        };
        if t.status == Status::Starting {
            if now >= t.lease_deadline {
                return false;
            }
            t.status = Status::Presenting;
            self.record(now, "started", json!({"taskId":id,"generation":generation}));
        }
        true
    }
    pub fn channel(
        &mut self,
        id: u64,
        generation: u64,
        channel: &str,
        failed: bool,
        now: u64,
    ) -> bool {
        if !self.valid(id, generation) {
            return false;
        }
        let t = self.tasks.iter_mut().find(|t| t.id == id).unwrap();
        if t.status != Status::Presenting {
            return false;
        }
        let Some(c) = t.channels.get_mut(channel) else {
            return false;
        };
        if c != "pending" {
            return false;
        }
        *c = if failed { "failed" } else { "completed" }.into();
        let done = t.channels.values().all(|s| s != "pending");
        let all_failed = t.channels.values().all(|s| s == "failed");
        self.record(
            now,
            "channel-ended",
            json!({"taskId":id,"channel":channel,"failed":failed}),
        );
        if done {
            self.finish(
                id,
                if all_failed {
                    Status::Failed
                } else {
                    Status::Completed
                },
                if all_failed {
                    "全部通道失败"
                } else {
                    "所有有限通道已收尾"
                },
                now,
            )
        }
        true
    }
    pub fn stop(&mut self, now: u64) {
        if let Some(t) = self.active() {
            self.finish(t.id, Status::Cancelled, "用户停止", now)
        }
        self.end_base_transition("cancelled", now);
    }
    pub fn revoke_owner(&mut self, owner: &str, now: u64) {
        self.claims.remove(owner);
        let ids: Vec<_> = self
            .tasks
            .iter()
            .filter(|t| t.owner == owner && !t.status.terminal())
            .map(|t| t.id)
            .collect();
        for id in ids {
            self.finish(id, Status::Cancelled, "用户关闭/卸载插件", now)
        }
        if self
            .base_transition
            .as_ref()
            .is_some_and(|t| t.owner == owner)
        {
            self.end_base_transition("cancelled", now);
        }
    }
    pub fn switch_instance(&mut self, id: &str, now: u64) {
        self.end_base_transition("cancelled", now);
        let ids: Vec<_> = self
            .tasks
            .iter()
            .filter(|t| !t.status.terminal())
            .map(|t| t.id)
            .collect();
        for id in ids {
            self.finish(id, Status::Cancelled, "用户切换角色", now)
        }
        self.current_instance = id.into();
        self.claims.clear();
        self.revisions.clear();
        self.dedupe.clear();
        self.desired_base = "idle".into();
        self.applied_base = "idle".into();
    }
    pub fn mute(&mut self, value: bool, now: u64) {
        self.muted = value;
        if value {
            if let Some(t) = self.active().cloned() {
                if t.status == Status::Presenting {
                    self.channel(t.id, t.generation, "audio", false, now);
                }
            }
        }
    }
    pub fn claim(&mut self, owner: &str, mode: &str, priority: Priority, suppress: bool) {
        if self
            .claims
            .get(owner)
            .is_some_and(|c| c.mode == mode && c.priority == priority && c.suppress == suppress)
        {
            return;
        }
        let sequence = self.seq();
        self.claims.insert(
            owner.into(),
            Claim {
                owner: owner.into(),
                mode: mode.into(),
                priority,
                sequence,
                suppress,
            },
        );
    }
    pub fn suppressing(&self) -> bool {
        self.claims.values().any(|c| c.suppress)
    }
    fn end_base_transition(&mut self, status: &str, now: u64) {
        if let Some(t) = self.base_transition.take() {
            // Failure/explicit cancellation falls back to the target loop too;
            // never queue the same broken or interrupted exit again.
            self.applied_base = t.target_base.clone();
            self.record(now, "base-transition-ended", json!({"id":t.id,"parentTaskId":t.parent_task_id,"from":t.from_base,"to":t.target_base,"status":status}));
        }
    }
    pub fn base_transition_ack(&mut self, id: u64, generation: u64, event: &str, now: u64) -> bool {
        let Some(t) = self.base_transition.as_mut().filter(|t| {
            t.id == id && t.generation == generation && t.instance_id == self.current_instance
        }) else {
            return false;
        };
        match event {
            "started" => {
                if t.status == Status::Starting {
                    if now >= t.lease_deadline {
                        return false;
                    }
                    t.status = Status::Presenting;
                    let parent = t.parent_task_id;
                    self.record(
                        now,
                        "base-transition-started",
                        json!({"id":id,"parentTaskId":parent}),
                    );
                }
                true
            }
            "completed" | "failed" if t.status == Status::Presenting => {
                self.end_base_transition(event, now);
                true
            }
            _ => false,
        }
    }
    /// Reconcile only the latest requested base. A started transition is immutable.
    pub fn effective_base(&self, model: &Value) -> String {
        let best = self.claims.values().max_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| b.sequence.cmp(&a.sequence))
        });
        let requested = best.map(|c| c.mode.as_str()).unwrap_or("idle");
        if model["baseModes"][requested].is_object() {
            requested.into()
        } else {
            "idle".into()
        }
    }
    pub fn reconcile_base(&mut self, model: &Value, now: u64) {
        let best = self
            .claims
            .values()
            .max_by(|a, b| {
                a.priority
                    .cmp(&b.priority)
                    .then_with(|| b.sequence.cmp(&a.sequence))
            })
            .cloned();
        let desired = self.effective_base(model);
        self.desired_base = desired.clone();
        if self.base_transition.is_some() {
            return;
        }
        // While a foreground action covers the base, retire its obsolete return
        // target directly. Once that action ended and exposed the base loop,
        // play the real exit on the free animation lane; keep text/audio intact.
        let foreground = self
            .active()
            .filter(|t| {
                t.status == Status::Presenting
                    && t.target_base.is_none()
                    && t.channels.contains_key("animation")
            })
            .cloned();
        if let Some(foreground) = foreground {
            if self.applied_base != "idle"
                && !self.claims.values().any(|c| c.mode == self.applied_base)
            {
                let from = self.applied_base.clone();
                let pending: Vec<_> = self
                    .tasks
                    .iter()
                    .filter(|t| t.target_base.is_some() && t.status.pending())
                    .map(|t| t.id)
                    .collect();
                for id in pending {
                    self.finish(id, Status::Cancelled, "互动覆盖的旧基础收尾已失效", now);
                }
                if foreground
                    .channels
                    .get("animation")
                    .is_some_and(|s| s != "pending")
                {
                    // The foreground action already returned to the old loop,
                    // so its visible music -> idle exit is now required.
                    let clips: Vec<String> = [
                        &model["baseModes"][&from]["exit"],
                        &model["baseModes"]["idle"]["enter"],
                    ]
                    .into_iter()
                    .filter_map(|v| v.as_str())
                    .filter(|id| model["clips"][*id].is_object())
                    .map(str::to_owned)
                    .collect();
                    if !clips.is_empty() {
                        let id = self.seq();
                        self.generation += 1;
                        self.base_transition = Some(BaseTransition {
                            id,
                            generation: self.generation,
                            instance_id: self.current_instance.clone(),
                            parent_task_id: foreground.id,
                            owner: foreground.owner,
                            from_base: from.clone(),
                            target_base: "idle".into(),
                            clips: clips.clone(),
                            status: Status::Starting,
                            lease_deadline: now + 5000,
                        });
                        self.record(now, "base-transition-planned", json!({"id":id,"parentTaskId":foreground.id,"from":from,"to":"idle","clips":clips}));
                        return;
                    }
                }
                self.applied_base = "idle".into();
                self.record(
                    now,
                    "base-fallback",
                    json!({"taskId":foreground.id,"from":from,"to":"idle"}),
                );
            }
        }
        let stale: Vec<_> = self
            .tasks
            .iter()
            .filter(|t| {
                t.target_base.is_some()
                    && t.status.pending()
                    && t.target_base.as_deref() != Some(&desired)
            })
            .map(|t| t.id)
            .collect();
        for id in stale {
            self.finish(id, Status::Cancelled, "合并过时的基础姿态目标", now)
        }
        if self.applied_base == desired
            || self
                .tasks
                .iter()
                .any(|t| !t.status.terminal() && t.target_base.as_deref() == Some(&desired))
        {
            return;
        }
        if self.active().is_some_and(|t| t.target_base.is_some()) {
            return;
        }
        let mut clips = vec![];
        for v in [
            &model["baseModes"][&self.applied_base]["exit"],
            &model["baseModes"][&desired]["enter"],
        ] {
            if let Some(s) = v.as_str() {
                if model["clips"][s].is_object() {
                    clips.push(s.to_owned())
                }
            }
        }
        let id = self.seq();
        self.generation += 1;
        let channels = if clips.is_empty() {
            BTreeMap::new()
        } else {
            BTreeMap::from([("animation".into(), "pending".into())])
        };
        self.tasks.push_back(Task {
            id,
            instance_id: self.current_instance.clone(),
            owner: "host.base".into(),
            revision: 0,
            sequence: id,
            generation: self.generation,
            priority: best.map(|c| c.priority).unwrap_or(Priority::Normal),
            status: Status::Ready,
            accepted_at: now,
            deadline: u64::MAX,
            prepare_deadline: u64::MAX,
            lease_deadline: 0,
            proactive: false,
            plan: json!({"clips":clips,"repeat":1}),
            channels,
            target_base: Some(desired),
            result: None,
        });
        self.trim();
    }
    pub fn tick(&mut self, now: u64) {
        if self
            .base_transition
            .as_ref()
            .is_some_and(|t| t.status == Status::Starting && now >= t.lease_deadline)
        {
            self.end_base_transition("start-timeout", now);
        }
        let invalid: Vec<_> = self
            .tasks
            .iter()
            .filter_map(|t| {
                if t.status.pending() {
                    if now >= t.deadline {
                        return Some((t.id, Status::Expired, "排队有效期已过"));
                    }
                    if t.instance_id != self.current_instance
                        || self
                            .revisions
                            .get(&t.owner)
                            .is_some_and(|r| *r != t.revision)
                    {
                        return Some((t.id, Status::Cancelled, "关联配置已失效"));
                    }
                    if t.status == Status::Preparing && now >= t.prepare_deadline {
                        return Some((t.id, Status::Failed, "准备超时"));
                    }
                }
                if t.status == Status::Starting && now >= t.lease_deadline {
                    return Some((t.id, Status::Failed, "开始授权超时，已撤销"));
                }
                None
            })
            .collect();
        for (id, status, reason) in invalid {
            self.finish(id, status, reason, now)
        }
        if self.active().is_some() || self.base_transition.is_some() {
            return;
        }
        let suppressed = self.suppressing();
        let head = self
            .tasks
            .iter()
            .filter(|t| t.status.pending() && !(suppressed && t.proactive))
            .max_by(|a, b| {
                a.priority
                    .cmp(&b.priority)
                    .then_with(|| b.sequence.cmp(&a.sequence))
            })
            .map(|t| (t.id, t.status, t.channels.is_empty()));
        if let Some((id, Status::Ready, empty)) = head {
            if empty {
                self.finish(id, Status::Completed, "基础姿态边界提交", now);
                return;
            }
            let t = self.tasks.iter_mut().find(|t| t.id == id).unwrap();
            t.status = Status::Starting;
            t.lease_deadline = now + 5_000;
            self.record(now, "lease-granted", json!({"taskId":id}));
        }
    }
    pub fn snapshot(&self) -> Value {
        json!({"active":self.active(),"tasks":self.tasks,"claims":self.claims.values().collect::<Vec<_>>(),
            "desiredBase":self.desired_base,"appliedBase":self.applied_base,"baseTransition":self.base_transition,"suppressed":self.suppressing(),"muted":self.muted,"log":self.log})
    }
    pub fn pending_owners(&self) -> BTreeSet<String> {
        self.tasks
            .iter()
            .filter(|t| t.status.pending())
            .map(|t| t.owner.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn req(p: Priority, preparing: bool) -> Request {
        Request {
            instance_id: "a".into(),
            owner: "p".into(),
            revision: 0,
            priority: p,
            proactive: true,
            ttl_ms: 10_000,
            preparing,
            plan: json!({}),
            channels: vec!["text".into(), "audio".into(), "animation".into()],
            dedupe: None,
        }
    }
    fn begin(s: &mut Scheduler, now: u64) -> Task {
        s.tick(now);
        let t = s.active().unwrap().clone();
        assert!(s.started(t.id, t.generation, now));
        t
    }
    fn complete(s: &mut Scheduler, t: &Task, now: u64) {
        for c in ["text", "audio", "animation"] {
            s.channel(t.id, t.generation, c, false, now);
        }
    }
    #[test]
    fn non_preemptive_whole_task_and_ttl_after_start() {
        let mut s = Scheduler::new("a");
        let id = s.submit(req(Priority::Background, false), 0).unwrap();
        let a = begin(&mut s, 0);
        s.submit(req(Priority::Important, false), 1).unwrap();
        s.channel(a.id, a.generation, "text", false, 2);
        s.channel(a.id, a.generation, "audio", false, 3);
        s.tick(10_001);
        assert_eq!(s.active().unwrap().id, id);
        s.channel(a.id, a.generation, "animation", false, 10_002);
        assert_eq!(s.tasks[0].status, Status::Completed);
        assert_eq!(s.tasks[1].status, Status::Expired);
    }
    #[test]
    fn fifo_preparation_and_higher_priority_bypass() {
        let mut s = Scheduler::new("a");
        let a = s.submit(req(Priority::Normal, true), 0).unwrap();
        s.submit(req(Priority::Normal, false), 1).unwrap();
        s.tick(2);
        assert!(s.active().is_none());
        let h = s.submit(req(Priority::Important, false), 3).unwrap();
        let t = begin(&mut s, 4);
        assert_eq!(t.id, h);
        complete(&mut s, &t, 5);
        let g = s.tasks[0].generation;
        assert!(s.ready(a, g, json!({}), vec!["text".into()], 6));
        assert_eq!(begin(&mut s, 7).id, a);
    }
    #[test]
    fn cancellation_and_late_callbacks_cannot_revive() {
        let mut s = Scheduler::new("a");
        s.submit(req(Priority::Normal, false), 0).unwrap();
        let t = begin(&mut s, 0);
        s.stop(1);
        assert!(!s.channel(t.id, t.generation, "audio", false, 2));
        assert!(!s.started(t.id, t.generation, 2));
        assert!(!s.ready(t.id, t.generation, json!({}), vec![], 2));
    }
    #[test]
    fn lease_timeout_rejects_late_started() {
        let mut s = Scheduler::new("a");
        s.submit(req(Priority::Normal, false), 0).unwrap();
        s.tick(0);
        let t = s.active().unwrap().clone();
        s.tick(5000);
        assert!(!s.started(t.id, t.generation, 5001));
        assert!(s.active().is_none());
    }
    #[test]
    fn duplicate_and_wrong_generation_receipts() {
        let mut s = Scheduler::new("a");
        s.submit(req(Priority::Normal, false), 0).unwrap();
        let t = begin(&mut s, 0);
        assert!(!s.channel(t.id, t.generation + 1, "text", false, 1));
        assert!(s.channel(t.id, t.generation, "text", false, 1));
        assert!(!s.channel(t.id, t.generation, "text", false, 1));
        complete(&mut s, &t, 2);
        assert_eq!(s.log.iter().filter(|v| v["event"] == "finished").count(), 1);
    }
    #[test]
    fn mute_only_audio_and_disable_only_owner() {
        let mut s = Scheduler::new("a");
        s.submit(req(Priority::Normal, false), 0).unwrap();
        let t = begin(&mut s, 0);
        s.mute(true, 1);
        assert_eq!(s.active().unwrap().channels["audio"], "completed");
        assert!(s.active().is_some());
        s.revoke_owner("other", 2);
        assert!(s.active().is_some());
        s.revoke_owner("p", 3);
        assert!(!s.valid(t.id, t.generation));
    }
    #[test]
    fn base_coalesces_and_restores_latest_media() {
        let mut s = Scheduler::new("a");
        let model = json!({"baseModes":{"idle":{},"music":{},"quiet":{}}});
        s.submit(req(Priority::Background, false), 0).unwrap();
        let t = begin(&mut s, 0);
        s.claim("media", "music", Priority::Normal, false);
        s.claim("fullscreen", "quiet", Priority::Important, true);
        s.reconcile_base(&model, 1);
        assert_eq!(s.applied_base, "idle");
        assert_eq!(s.desired_base, "quiet");
        assert!(s.suppressing());
        s.claims.remove("fullscreen");
        s.reconcile_base(&model, 2);
        complete(&mut s, &t, 3);
        s.tick(4);
        assert_eq!(s.applied_base, "music");
        assert!(!s
            .tasks
            .iter()
            .any(|t| t.target_base.as_deref() == Some("quiet") && t.status == Status::Completed));
    }
    #[test]
    fn base_transition_does_not_jump_user_or_interrupt_started_transition() {
        let mut s = Scheduler::new("a");
        let model = json!({"clips":{"enter":{}},"baseModes":{"idle":{},"quiet":{"enter":"enter"}}});
        s.claim("full", "quiet", Priority::Important, true);
        s.reconcile_base(&model, 0);
        let mut r = req(Priority::User, false);
        r.owner = "host.user".into();
        r.proactive = false;
        let u = s.submit(r, 1).unwrap();
        let t = begin(&mut s, 2);
        assert_eq!(t.id, u);
        complete(&mut s, &t, 3);
        let base = begin(&mut s, 4);
        s.claims.clear();
        s.reconcile_base(&model, 5);
        assert_eq!(s.active().unwrap().id, base.id);
        s.channel(base.id, base.generation, "animation", false, 6);
        s.reconcile_base(&model, 7);
        s.tick(7);
        assert_eq!(s.applied_base, "idle");
    }
    fn music_scene() -> (Scheduler, Value) {
        let mut s = Scheduler::new("a");
        let model = json!({"clips":{"music_exit":{},"quiet_enter":{}},"baseModes":{"idle":{},"music":{"exit":"music_exit"},"quiet":{"enter":"quiet_enter"}}});
        s.claim("media", "music", Priority::Normal, false);
        s.reconcile_base(&model, 0);
        s.tick(0);
        assert_eq!(s.applied_base, "music");
        (s, model)
    }
    #[test]
    fn stopped_media_retires_action_return_pose_without_interrupting_whole_task() {
        let (mut s, model) = music_scene();
        s.submit(req(Priority::Background, false), 1).unwrap();
        let action = begin(&mut s, 2);
        let next = s.submit(req(Priority::Important, false), 3).unwrap();
        s.claims.remove("media");
        s.reconcile_base(&model, 4);
        assert_eq!(s.applied_base, "idle");
        assert_eq!(s.active().unwrap().id, action.id);
        assert!(s
            .active()
            .unwrap()
            .channels
            .values()
            .all(|s| s == "pending"));
        assert!(!s
            .tasks
            .iter()
            .any(|t| t.target_base.is_some() && !t.status.terminal()));
        s.channel(action.id, action.generation, "animation", false, 5);
        s.tick(6);
        assert_eq!(s.active().unwrap().id, action.id);
        s.channel(action.id, action.generation, "audio", false, 7);
        s.tick(8);
        assert_eq!(s.active().unwrap().id, action.id);
        s.channel(action.id, action.generation, "text", false, 9);
        assert_eq!(begin(&mut s, 10).id, next);
    }
    #[test]
    fn media_stops_after_action_ended_while_text_still_holds_task() {
        let (mut s, model) = music_scene();
        s.submit(req(Priority::Normal, false), 1).unwrap();
        let action = begin(&mut s, 2);
        s.channel(action.id, action.generation, "animation", false, 3);
        s.claims.remove("media");
        s.reconcile_base(&model, 4);
        assert_eq!(s.applied_base, "music");
        let exit = s.base_transition.clone().unwrap();
        assert_eq!(exit.clips, ["music_exit"]);
        assert!(s.base_transition_ack(exit.id, exit.generation, "started", 5));
        assert_eq!(s.active().unwrap().channels["text"], "pending");
        assert_eq!(s.active().unwrap().channels["audio"], "pending");
        assert!(s.base_transition_ack(exit.id, exit.generation, "completed", 6));
        assert_eq!(s.applied_base, "idle");
        assert_eq!(s.active().unwrap().id, action.id);
        assert!(!s
            .tasks
            .iter()
            .any(|t| t.plan["clips"] == json!(["music_exit"])));
    }
    fn pending_base_exit() -> (Scheduler, Value, Task, BaseTransition) {
        let (mut s, model) = music_scene();
        s.submit(req(Priority::Normal, false), 1).unwrap();
        let task = begin(&mut s, 2);
        s.channel(task.id, task.generation, "animation", false, 3);
        s.claims.remove("media");
        s.reconcile_base(&model, 4);
        let exit = s.base_transition.clone().unwrap();
        (s, model, task, exit)
    }
    #[test]
    fn base_exit_holds_next_reply_when_parent_finishes_first() {
        let (mut s, _, task, exit) = pending_base_exit();
        assert!(s.base_transition_ack(exit.id, exit.generation, "started", 5));
        s.submit(req(Priority::Important, false), 6).unwrap();
        complete(&mut s, &task, 7);
        s.tick(8);
        assert!(s.active().is_none());
        assert!(s.base_transition.is_some());
        assert!(s.base_transition_ack(exit.id, exit.generation, "completed", 9));
        assert!(s.base_transition.is_none());
        assert_eq!(s.applied_base, "idle");
        assert_ne!(begin(&mut s, 10).id, task.id);
    }
    #[test]
    fn stop_switch_and_owner_revoke_invalidate_base_exit_callbacks() {
        for action in ["stop", "switch", "revoke"] {
            let (mut s, _, _, exit) = pending_base_exit();
            assert!(s.base_transition_ack(exit.id, exit.generation, "started", 5));
            match action {
                "stop" => s.stop(6),
                "switch" => s.switch_instance("b", 6),
                _ => s.revoke_owner("p", 6),
            }
            assert!(s.base_transition.is_none());
            assert_eq!(s.applied_base, "idle");
            assert!(!s.base_transition_ack(exit.id, exit.generation, "completed", 7));
        }
    }
    #[test]
    fn base_exit_timeout_or_failure_preserves_pending_text_and_does_not_retry() {
        for timeout in [true, false] {
            let (mut s, model, task, exit) = pending_base_exit();
            if timeout {
                s.tick(5004);
            } else {
                assert!(s.base_transition_ack(exit.id, exit.generation, "started", 5));
                assert!(s.base_transition_ack(exit.id, exit.generation, "failed", 6));
            }
            assert!(s.base_transition.is_none());
            assert_eq!(s.applied_base, "idle");
            assert_eq!(s.active().unwrap().id, task.id);
            assert_eq!(s.active().unwrap().channels["text"], "pending");
            s.reconcile_base(&model, 5005);
            assert!(s.base_transition.is_none());
        }
    }
    #[test]
    fn resumed_media_does_not_interrupt_the_started_base_exit() {
        let (mut s, model, task, exit) = pending_base_exit();
        s.base_transition_ack(exit.id, exit.generation, "started", 5);
        s.claim("media", "music", Priority::Normal, false);
        s.reconcile_base(&model, 6);
        assert_eq!(s.base_transition.as_ref().unwrap().id, exit.id);
        s.base_transition_ack(exit.id, exit.generation, "completed", 7);
        s.reconcile_base(&model, 8);
        s.tick(8);
        assert_eq!(s.applied_base, "idle");
        assert_eq!(s.active().unwrap().id, task.id);
        complete(&mut s, &task, 9);
        s.tick(10);
        assert_eq!(s.applied_base, "music");
    }
    #[test]
    fn missing_base_exit_clip_uses_idle_without_adding_a_lock() {
        let (mut s, mut model) = music_scene();
        model["baseModes"]["music"]
            .as_object_mut()
            .unwrap()
            .remove("exit");
        s.submit(req(Priority::Normal, false), 1).unwrap();
        let task = begin(&mut s, 2);
        s.channel(task.id, task.generation, "animation", false, 3);
        s.claims.remove("media");
        s.reconcile_base(&model, 4);
        assert_eq!(s.applied_base, "idle");
        assert!(s.base_transition.is_none());
        assert_eq!(s.active().unwrap().id, task.id);
    }
    #[test]
    fn base_exit_receipts_require_their_own_token_and_finish_once() {
        let (mut s, _, task, exit) = pending_base_exit();
        assert!(!s.base_transition_ack(task.id, task.generation, "started", 5));
        assert!(!s.base_transition_ack(exit.id, exit.generation + 1, "started", 5));
        assert!(!s.base_transition_ack(exit.id, exit.generation, "completed", 5));
        assert!(s.base_transition_ack(exit.id, exit.generation, "started", 6));
        assert!(s.base_transition_ack(exit.id, exit.generation, "completed", 7));
        assert!(!s.base_transition_ack(exit.id, exit.generation, "completed", 8));
        assert_eq!(
            s.log
                .iter()
                .filter(|e| e["event"] == "base-transition-ended")
                .count(),
            1
        );
    }
    #[test]
    fn stale_exit_is_rebuilt_without_old_music_clip_but_new_quiet_waits() {
        let (mut s, model) = music_scene();
        s.claims.remove("media");
        s.reconcile_base(&model, 1);
        let old_exit = s.tasks.back().unwrap().id;
        let mut request = req(Priority::User, false);
        request.owner = "host.user".into();
        request.proactive = false;
        s.submit(request, 2).unwrap();
        let action = begin(&mut s, 3);
        s.claim("fullscreen", "quiet", Priority::Important, true);
        s.reconcile_base(&model, 4);
        assert_eq!(s.applied_base, "idle");
        assert_eq!(
            s.tasks.iter().find(|t| t.id == old_exit).unwrap().status,
            Status::Cancelled
        );
        let quiet = s
            .tasks
            .iter()
            .find(|t| !t.status.terminal() && t.target_base.as_deref() == Some("quiet"))
            .unwrap();
        assert_eq!(quiet.plan["clips"], json!(["quiet_enter"]));
        assert_eq!(s.active().unwrap().id, action.id);
        complete(&mut s, &action, 5);
        let transition = begin(&mut s, 6);
        assert_eq!(transition.target_base.as_deref(), Some("quiet"));
    }
    #[test]
    fn valid_music_claim_is_preserved_under_foreground_action() {
        let (mut s, model) = music_scene();
        s.submit(req(Priority::Normal, false), 1).unwrap();
        let action = begin(&mut s, 2);
        s.reconcile_base(&model, 3);
        s.channel(action.id, action.generation, "animation", false, 4);
        s.reconcile_base(&model, 5);
        assert_eq!(s.applied_base, "music");
        assert!(s.log.iter().all(|e| e["event"] != "base-fallback"));
    }
    #[test]
    fn music_exit_still_plays_when_no_interaction_action_covers_it() {
        let (mut s, model) = music_scene();
        s.claims.remove("media");
        s.reconcile_base(&model, 1);
        let exit = begin(&mut s, 2);
        assert_eq!(exit.plan["clips"], json!(["music_exit"]));
        assert_eq!(s.applied_base, "music");
        s.reconcile_base(&model, 3);
        assert_eq!(s.active().unwrap().id, exit.id);
        s.channel(exit.id, exit.generation, "animation", false, 4);
        assert_eq!(s.applied_base, "idle");
    }
    #[test]
    fn no_user_impersonation_and_role_switch_invalidates() {
        let mut s = Scheduler::new("a");
        assert!(s.submit(req(Priority::User, false), 0).is_err());
        s.submit(req(Priority::Normal, false), 0).unwrap();
        let t = begin(&mut s, 0);
        s.switch_instance("b", 1);
        assert!(!s.valid(t.id, t.generation));
    }
    #[test]
    fn preparation_timeout_and_revision_revalidation() {
        let mut s = Scheduler::new("a");
        let mut r = req(Priority::Normal, true);
        r.ttl_ms = 60_000;
        s.submit(r, 0).unwrap();
        s.tick(30_000);
        assert_eq!(s.tasks[0].status, Status::Failed);
        s.submit(req(Priority::Normal, false), 30_001).unwrap();
        s.revisions.insert("p".into(), 1);
        s.tick(30_002);
        assert_eq!(s.tasks[1].status, Status::Cancelled);
    }
}
