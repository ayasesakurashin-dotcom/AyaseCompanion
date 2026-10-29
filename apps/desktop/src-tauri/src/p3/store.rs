use crate::{
    memory::Memory,
    p3::{
        calendar::{Dataset, SpecialDate},
        Observation,
    },
};
use rusqlite::params;
use serde_json::Value;
impl Memory {
    pub fn record_observation(
        &self,
        instance: &str,
        epoch: u64,
        o: &Observation,
    ) -> Result<bool, String> {
        let mut db = self.db(instance)?;
        let tx = db.transaction().map_err(|_| "观察事务失败")?;
        tx.execute("UPDATE observation_meta SET next_seq=next_seq+1 WHERE id=1 AND (SELECT epoch FROM meta WHERE id=1)=?1",[epoch]).map_err(|_|"观察序号更新失败")?;
        let n=tx.execute("INSERT OR IGNORE INTO observations(rowid,id,epoch,payload) SELECT (SELECT next_seq FROM observation_meta WHERE id=1),?1,?2,?3 WHERE (SELECT epoch FROM meta WHERE id=1)=?2",params![o.id,epoch,serde_json::to_string(o).map_err(|_|"观察编码失败")?]).map_err(|_|"观察保存失败")?;
        tx.commit().map_err(|_| "观察提交失败")?;
        Ok(n > 0)
    }
    pub fn observations(&self, instance: &str) -> Result<Vec<Observation>, String> {
        self.observation_page(instance, u64::MAX)
    }
    pub fn observation_page(
        &self,
        instance: &str,
        before: u64,
    ) -> Result<Vec<Observation>, String> {
        let db = self.db(instance)?;
        let mut s=db.prepare("SELECT rowid,payload FROM observations WHERE epoch=(SELECT epoch FROM meta WHERE id=1) AND rowid<?1 ORDER BY rowid DESC LIMIT 30").map_err(|_|"观察查询失败")?;
        let rows = s
            .query_map([before.min(i64::MAX as u64) as i64], |r| {
                Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|_| "观察查询失败")?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "观察读取失败")?;
        rows.into_iter()
            .map(|(seq, s)| {
                let mut o: Observation = serde_json::from_str(&s).map_err(|_| "观察记录损坏")?;
                o.seq = seq;
                Ok(o)
            })
            .collect()
    }
    pub fn observation_by_id(&self, instance: &str, id: &str) -> Result<Observation, String> {
        let db = self.db(instance)?;
        let (seq,raw):(u64,String)=db.query_row("SELECT rowid,payload FROM observations WHERE id=?1 AND epoch=(SELECT epoch FROM meta WHERE id=1)",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|"该观察已不存在")?;
        let mut o: Observation = serde_json::from_str(&raw).map_err(|_| "观察记录损坏")?;
        o.seq = seq;
        Ok(o)
    }
    pub fn observation_summary(&self, instance: &str) -> Result<Value, String> {
        let db = self.db(instance)?;
        db.query_row("SELECT summary,watermark,revision,(SELECT count(*) FROM observations WHERE epoch=(SELECT epoch FROM meta WHERE id=1)) FROM observation_meta WHERE id=1",[],|r|Ok(serde_json::json!({"summary":r.get::<_,String>(0)?,"watermark":r.get::<_,u64>(1)?,"revision":r.get::<_,u64>(2)?,"count":r.get::<_,u64>(3)?}))).map_err(|_|"观察摘要不可读".into())
    }
    pub fn observation_summary_input(
        &self,
        instance: &str,
    ) -> Result<Option<(u64, u64, u64, u64, String)>, String> {
        let db = self.db(instance)?;
        let metadata = self.observation_summary(instance)?;
        let old = metadata["watermark"].as_u64().unwrap_or(0);
        let mut s=db.prepare("SELECT rowid,payload FROM observations WHERE epoch=(SELECT epoch FROM meta WHERE id=1) AND rowid>?1 ORDER BY rowid LIMIT 16").map_err(|_|"观察摘要查询失败")?;
        let rows = s
            .query_map([old], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?)))
            .map_err(|_| "观察摘要查询失败")?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "观察摘要读取失败")?;
        if rows.len() < 12 {
            return Ok(None);
        }
        let through = rows.last().unwrap().0;
        let mut parts = vec![];
        for (seq, raw) in rows {
            let o: Observation =
                serde_json::from_str(&raw).map_err(|_| "观察记录损坏，原文已保留")?;
            parts.push(format!(
                "观察#{seq} {} {}；摘要:{}；可见字:{}",
                o.observed_at,
                o.label,
                crate::memory::bounded(&o.summary, 100),
                crate::memory::bounded(&o.visible_text, 120)
            ));
        }
        let input = format!(
            "旧摘要：{}\n{}",
            metadata["summary"].as_str().unwrap_or(""),
            parts.join("\n")
        );
        Ok(Some((
            self.epoch(instance)?,
            metadata["revision"].as_u64().unwrap_or(0),
            old,
            through,
            input,
        )))
    }
    pub fn commit_observation_summary(
        &self,
        instance: &str,
        epoch: u64,
        revision: u64,
        old: u64,
        through: u64,
        text: &str,
    ) -> Result<bool, String> {
        if text.trim().is_empty() || text.chars().count() > 1200 {
            return Err("观察摘要长度无效".into());
        }
        let db = self.db(instance)?;
        let n=db.execute("UPDATE observation_meta SET summary=?1,watermark=?2 WHERE id=1 AND revision=?3 AND watermark=?4 AND (SELECT epoch FROM meta WHERE id=1)=?5",params![text,through,revision,old,epoch]).map_err(|_|"观察摘要提交失败")?;
        Ok(n > 0)
    }
    pub fn remove_observation(&self, instance: &str, id: &str) -> Result<(), String> {
        let mut db = self.db(instance)?;
        let tx = db.transaction().map_err(|_| "观察删除事务失败")?;
        tx.execute("DELETE FROM observations WHERE id=?1", [id])
            .map_err(|_| "观察删除失败")?;
        tx.execute("UPDATE turn_evidence SET payload=json_remove(payload,'$.observation','$.facts.historicalObservation') WHERE json_extract(payload,'$.observation.id')=?1 OR json_extract(payload,'$.facts.historicalObservation.id')=?1",[id]).map_err(|_|"清除观察引用失败")?;
        tx.execute(
            "UPDATE observation_meta SET summary='',watermark=0,revision=revision+1",
            [],
        )
        .map_err(|_| "失效观察摘要失败")?;
        tx.commit().map_err(|_| "观察删除提交失败")?;
        Ok(())
    }
    fn p3_config(&self, instance: &str) -> Result<rusqlite::Connection, String> {
        let db = self.db(instance)?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS p3_config(id TEXT PRIMARY KEY,payload TEXT NOT NULL);",
        )
        .map_err(|_| "功能档案不可用")?;
        Ok(db)
    }
    pub fn calendar_data(&self, instance: &str) -> Result<Vec<Dataset>, String> {
        self.read_p3(instance, "calendar")
    }
    pub fn special_dates(&self, instance: &str) -> Result<Vec<SpecialDate>, String> {
        self.read_p3(instance, "special")
    }
    fn read_p3<T: serde::de::DeserializeOwned + Default>(
        &self,
        instance: &str,
        key: &str,
    ) -> Result<T, String> {
        let db = self.p3_config(instance)?;
        match db.query_row("SELECT payload FROM p3_config WHERE id=?1", [key], |r| {
            r.get::<_, String>(0)
        }) {
            Ok(s) => serde_json::from_str(&s).map_err(|_| "功能档案格式错误".into()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(T::default()),
            Err(_) => Err("功能档案读取失败".into()),
        }
    }
    pub fn save_calendar(&self, instance: &str, set: &Dataset) -> Result<(), String> {
        set.validate()?;
        let mut data = self.calendar_data(instance)?;
        data.retain(|d| d.country != set.country || d.year != set.year);
        if data.len() >= 20 {
            return Err("日历最多保留20组国家年份，请先删除旧数据".into());
        }
        data.push(set.clone());
        self.write_p3(instance, "calendar", &serde_json::to_value(data).unwrap())
    }
    pub fn save_special(&self, instance: &str, dates: &[SpecialDate]) -> Result<(), String> {
        if dates.len() > 100 {
            return Err("特殊日期最多100项".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for d in dates {
            d.validate()?;
            uuid::Uuid::parse_str(&d.id).map_err(|_| "特殊日期ID无效")?;
            if !ids.insert(&d.id) {
                return Err("特殊日期ID重复".into());
            }
        }
        self.write_p3(instance, "special", &serde_json::to_value(dates).unwrap())
    }
    pub fn clear_calendar(&self, instance: &str) -> Result<(), String> {
        self.write_p3(instance, "calendar", &serde_json::json!([]))
    }
    fn write_p3(&self, instance: &str, key: &str, value: &Value) -> Result<(), String> {
        self.p3_config(instance)?.execute("INSERT INTO p3_config VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![key,value.to_string()]).map_err(|_|"功能档案保存失败")?;
        Ok(())
    }
}
