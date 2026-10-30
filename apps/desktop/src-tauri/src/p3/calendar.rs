//! Explicit imports produce dated facts. No network calls during a calendar tick.
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Event {
    pub date: NaiveDate,
    pub name: String,
    pub category: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lunar {
    pub date: NaiveDate,
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub leap: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dataset {
    pub country: String,
    pub year: i32,
    pub source: String,
    pub imported_at: String,
    pub events: Vec<Event>,
    pub lunar: Vec<Lunar>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecialDate {
    pub id: String,
    pub name: String,
    pub calendar: String,
    pub year: Option<i32>,
    pub month: u32,
    pub day: u32,
    pub yearly: bool,
    pub leap: bool,
    pub enabled: bool,
}
impl Default for SpecialDate {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            calendar: "gregorian".into(),
            year: None,
            month: 1,
            day: 1,
            yearly: true,
            leap: false,
            enabled: true,
        }
    }
}
impl SpecialDate {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.chars().count() > 80
            || !["gregorian", "lunar"].contains(&self.calendar.as_str())
            || self.year.is_some_and(|y| !(1901..=2100).contains(&y))
            || (!self.yearly && self.year.is_none())
            || !(1..=12).contains(&self.month)
            || !(1..=31).contains(&self.day)
            || (self.calendar == "lunar" && self.day > 30)
            || (self.calendar == "gregorian"
                && (self.leap
                    || NaiveDate::from_ymd_opt(
                        if self.yearly {
                            2000
                        } else {
                            self.year.unwrap_or(2000)
                        },
                        self.month,
                        self.day,
                    )
                    .is_none()))
        {
            return Err("特殊日期的名称、历法、日期或循环规则无效".into());
        }
        Ok(())
    }
    pub fn occurs(&self, date: NaiveDate, data: &[Dataset]) -> bool {
        self.enabled
            && if self.calendar == "gregorian" {
                self.month == date.month()
                    && self.day == date.day()
                    && (self.yearly || self.year == Some(date.year()))
            } else {
                data.iter().flat_map(|d| &d.lunar).any(|d| {
                    d.date == date
                        && d.month == self.month
                        && d.day == self.day
                        && d.leap == self.leap
                        && (self.yearly || self.year == Some(d.year))
                })
            }
    }
}
impl Dataset {
    pub fn validate(&self) -> Result<(), String> {
        if self.country.len() != 2
            || !self.country.bytes().all(|b| b.is_ascii_uppercase())
            || !(1902..=2100).contains(&self.year)
            || self.source.len() > 500
            || self.events.len() > 800
            || self.lunar.len() > 366
            || self.events.iter().any(|e| {
                e.date.year() != self.year
                    || e.name.trim().is_empty()
                    || e.name.chars().count() > 200
                    || !["traditional", "public", "solar-term", "workday"]
                        .contains(&e.category.as_str())
            })
            || self.lunar.iter().any(|d| {
                d.date.year() != self.year
                    || !(1..=12).contains(&d.month)
                    || !(1..=30).contains(&d.day)
            })
        {
            return Err("日历数据无效".into());
        }
        Ok(())
    }
    pub fn today(&self, date: NaiveDate) -> Vec<Value> {
        self.events.iter().filter(|e|e.date==date).map(|e|json!({"date":e.date,"name":e.name,"category":e.category,"source":self.source,"importedAt":self.imported_at,"country":self.country})).collect()
    }
}
fn download(url: &str) -> Result<String, String> {
    let r = crate::services::client(20)?
        .get(url)
        .send()
        .map_err(|_| "日历来源暂不可用")?;
    if !r.status().is_success() {
        return Err("该国家年份日历暂不可用".into());
    }
    let mut bytes = vec![];
    r.take(1_000_001)
        .read_to_end(&mut bytes)
        .map_err(|_| "日历下载未完成")?;
    if bytes.len() > 1_000_000 {
        return Err("日历资料过大".into());
    }
    String::from_utf8(bytes).map_err(|_| "日历编码无效".into())
}
pub fn years(start: i32, end: i32) -> Result<std::ops::RangeInclusive<i32>, String> {
    if !(1902..=2100).contains(&start) || !(start..=2100).contains(&end) || end - start >= 20 {
        return Err(
            "起止年份需在1902—2100年之间，结束年不早于开始年，一次最多20年（包含两端）".into(),
        );
    }
    Ok(start..=end)
}
pub fn import_range(country: &str, start: i32, end: i32) -> Result<Vec<Dataset>, String> {
    years(start, end)?
        .map(|year| {
            import(country, year)
                .map_err(|e| format!("{country} {year}年：{e}。本次区间未写入，已有日历保留"))
        })
        .collect()
}
pub fn import(country: &str, year: i32) -> Result<Dataset, String> {
    let country = country.to_ascii_uppercase();
    if country.len() != 2
        || !country.bytes().all(|b| b.is_ascii_uppercase())
        || !(1902..=2100).contains(&year)
    {
        return Err("请选择两位国家代码与1902—2100年".into());
    }
    if country == "CN" {
        let previous = download(&format!(
            "https://www.hko.gov.hk/en/gts/time/calendar/text/files/T{}e.txt",
            year - 1
        ))?;
        let current = download(&format!(
            "https://www.hko.gov.hk/en/gts/time/calendar/text/files/T{year}e.txt"
        ))?;
        return parse_hko(year, &current, Some(&previous));
    }
    let url = format!("https://date.nager.at/api/v3/PublicHolidays/{year}/{country}");
    let raw: Vec<Value> =
        serde_json::from_str(&download(&url)?).map_err(|_| "公众假日资料格式无效")?;
    let mut set = Dataset {
        country: country.clone(),
        year,
        source: url,
        imported_at: chrono::Utc::now().to_rfc3339(),
        events: vec![],
        lunar: vec![],
    };
    if raw.len() > 500 {
        return Err("日历事件过多".into());
    }
    for row in raw {
        if row["countryCode"] != country {
            return Err("日历国家不匹配".into());
        }
        if row["global"] != true
            || !row["types"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == "Public"))
        {
            continue;
        }
        let date = NaiveDate::parse_from_str(row["date"].as_str().unwrap_or(""), "%Y-%m-%d")
            .map_err(|_| "日历日期无效")?;
        let name = row["localName"]
            .as_str()
            .or(row["name"].as_str())
            .unwrap_or("");
        set.events.push(Event {
            date,
            name: name.into(),
            category: "public".into(),
        });
    }
    if set.events.is_empty() {
        return Err("该来源没有此年份的全国公众假日，未修改日历".into());
    }
    set.validate()?;
    Ok(set)
}
fn rows(text: &str) -> Vec<(NaiveDate, Vec<&str>)> {
    text.lines()
        .filter_map(|l| {
            let c = l.split_whitespace().collect::<Vec<_>>();
            NaiveDate::parse_from_str(c.first()?, "%Y/%m/%d")
                .ok()
                .map(|d| (d, c))
        })
        .collect()
}
fn month_header(raw: &str) -> Option<u32> {
    let num = raw
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    num.parse::<u32>().ok().filter(|n| (1..=12).contains(n))
}
pub fn parse_hko(year: i32, current: &str, previous: Option<&str>) -> Result<Dataset, String> {
    let mut month = 12;
    let mut leap = false;
    let mut ly = year - 1;
    if let Some(previous) = previous {
        for (_, c) in rows(previous) {
            if c.get(2) == Some(&"Lunar") {
                let next = month_header(c[1]).ok_or("农历月份无效")?;
                leap = month == next;
                month = next;
            }
        }
    }
    let rows = rows(current);
    let expected = (NaiveDate::from_ymd_opt(year + 1, 1, 1).ok_or("年份无效")?
        - NaiveDate::from_ymd_opt(year, 1, 1).ok_or("年份无效")?)
    .num_days() as usize;
    if rows.len() != expected
        || rows
            .iter()
            .enumerate()
            .any(|(i, (d, _))| d.year() != year || d.ordinal() as usize != i + 1)
    {
        return Err("公农历对照表不完整，未导入".into());
    }
    let mut set = Dataset {
        country: "CN".into(),
        year,
        source: format!("https://www.hko.gov.hk/en/gts/time/calendar/text/files/T{year}e.txt"),
        imported_at: chrono::Utc::now().to_rfc3339(),
        events: vec![],
        lunar: vec![],
    };
    let terms = [
        ("Minor Cold", "小寒"),
        ("Moderate Cold", "小寒"),
        ("Severe Cold", "大寒"),
        ("Spring Commences", "立春"),
        ("Spring Showers", "雨水"),
        ("Insects Waken", "惊蛰"),
        ("Vernal Equinox", "春分"),
        ("Bright & Clear", "清明"),
        ("Corn Rain", "谷雨"),
        ("Summer Commences", "立夏"),
        ("Corn Forms", "小满"),
        ("Corn on Ear", "芒种"),
        ("Summer Solstice", "夏至"),
        ("Moderate Heat", "小暑"),
        ("Great Heat", "大暑"),
        ("Autumn Commences", "立秋"),
        ("End of Heat", "处暑"),
        ("White Dew", "白露"),
        ("Autumnal Equinox", "秋分"),
        ("Cold Dew", "寒露"),
        ("Frost", "霜降"),
        ("Winter Commences", "立冬"),
        ("Light Snow", "小雪"),
        ("Heavy Snow", "大雪"),
        ("Winter Solstice", "冬至"),
    ];
    for (date, c) in rows {
        let day = if c.get(2) == Some(&"Lunar") {
            let next = month_header(c[1]).ok_or("农历月份无效")?;
            leap = next == month;
            month = next;
            1
        } else {
            c.get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|n| (1..=30).contains(n))
                .ok_or("农历日无效")?
        };
        if month == 1 && day == 1 && !leap {
            ly = year;
        }
        set.lunar.push(Lunar {
            date,
            year: ly,
            month,
            day,
            leap,
        });
        let name = if !leap {
            match (month, day) {
                (1, 1) => Some("春节"),
                (1, 15) => Some("元宵节"),
                (5, 5) => Some("端午节"),
                (7, 7) => Some("七夕"),
                (8, 15) => Some("中秋节"),
                (9, 9) => Some("重阳节"),
                (12, 8) => Some("腊八节"),
                _ => None,
            }
        } else {
            None
        };
        if let Some(name) = name {
            set.events.push(Event {
                date,
                name: name.into(),
                category: "traditional".into(),
            });
        }
        if month == 1 && day == 1 && !leap {
            if let Some(eve) = date.pred_opt().filter(|d| d.year() == year) {
                set.events.push(Event {
                    date: eve,
                    name: "除夕".into(),
                    category: "traditional".into(),
                });
            }
        }
        let line = c.join(" ");
        if let Some((_, name)) = terms.iter().find(|(en, _)| line.ends_with(en)) {
            set.events.push(Event {
                date,
                name: (*name).into(),
                category: if *name == "清明" {
                    "traditional"
                } else {
                    "solar-term"
                }
                .into(),
            });
        }
    }
    set.validate()?;
    Ok(set)
}
