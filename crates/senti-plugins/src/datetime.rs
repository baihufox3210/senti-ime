//! Inserts the current date / time / weekday on short trigger codes.

use senti_core::{
    Candidate, CandidateSource, InputMode, LocalDateTime, Plugin, PluginManifest, PredictionQuery,
};

const ID: &str = "datetime";
/// Plugin results should appear first but not drown exact learned words.
const BASE_SCORE: f64 = 1.45;

const WEEKDAYS_ZH: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];
const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    Date,
    Time,
    Weekday,
}

fn trigger(mode: InputMode, input: &str) -> Option<Trigger> {
    let lower = input.to_lowercase();
    match (mode, lower.as_str()) {
        (InputMode::Chinese, "rq" | "riqi") | (InputMode::English, "date" | "today") => {
            Some(Trigger::Date)
        }
        (InputMode::Chinese, "sj" | "shijian") | (InputMode::English, "time" | "now") => {
            Some(Trigger::Time)
        }
        (InputMode::Chinese, "xq" | "xingqi") | (InputMode::English, "weekday") => {
            Some(Trigger::Weekday)
        }
        _ => None,
    }
}

fn format(trigger: Trigger, mode: InputMode, now: &LocalDateTime) -> Vec<String> {
    let weekday_zh = WEEKDAYS_ZH[now.weekday.min(6) as usize];
    let LocalDateTime {
        year,
        month,
        day,
        hour,
        minute,
        ..
    } = *now;
    match (trigger, mode) {
        (Trigger::Date, InputMode::Chinese | InputMode::Zhuyin | InputMode::Mixed) => vec![
            format!("{year}年{month}月{day}日"),
            format!("{year}-{month:02}-{day:02}"),
            format!("{month}月{day}日 星期{weekday_zh}"),
        ],
        (Trigger::Date, InputMode::English) => vec![
            format!("{year}-{month:02}-{day:02}"),
            format!(
                "{} {day}, {year}",
                MONTHS_EN[(month.clamp(1, 12) - 1) as usize]
            ),
        ],
        (Trigger::Time, InputMode::Chinese | InputMode::Zhuyin | InputMode::Mixed) => {
            let period = match hour {
                0..=5 => "凌晨",
                6..=11 => "上午",
                12 => "中午",
                13..=17 => "下午",
                _ => "晚上",
            };
            let hour12 = if hour % 12 == 0 { 12 } else { hour % 12 };
            vec![
                format!("{hour:02}:{minute:02}"),
                format!("{period}{hour12}點{minute:02}分"),
            ]
        }
        (Trigger::Time, InputMode::English) => vec![format!("{hour:02}:{minute:02}")],
        (Trigger::Weekday, InputMode::Chinese | InputMode::Zhuyin | InputMode::Mixed) => {
            vec![format!("星期{weekday_zh}"), format!("週{weekday_zh}")]
        }
        (Trigger::Weekday, InputMode::English) => {
            const DAYS: [&str; 7] = [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ];
            vec![DAYS[now.weekday.min(6) as usize].to_owned()]
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DateTimePlugin;

impl Plugin for DateTimePlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: ID,
            name: "日期時間",
            description: "輸入 rq / sj / xq（英文模式：date / time / weekday）插入目前日期、時間或星期。",
            enabled_by_default: true,
        }
    }

    fn suggest(&self, query: &PredictionQuery<'_>) -> Vec<Candidate> {
        let Some(trigger) = trigger(query.mode, query.input) else {
            return Vec::new();
        };
        format(trigger, query.mode, &query.context.now)
            .into_iter()
            .enumerate()
            .map(|(i, text)| {
                Candidate::new(
                    text,
                    query.input.len(),
                    BASE_SCORE - 0.01 * i as f64,
                    CandidateSource::Plugin(ID.to_owned()),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_query;
    use senti_core::InputContext;

    fn suggest(mode: InputMode, input: &str) -> Vec<String> {
        let context = InputContext {
            now: LocalDateTime {
                year: 2026,
                month: 11,
                day: 19,
                hour: 21,
                minute: 5,
                weekday: 3,
            },
            ..InputContext::default()
        };
        with_query(input, mode, context, |q| {
            DateTimePlugin
                .suggest(q)
                .into_iter()
                .map(|c| c.text)
                .collect()
        })
    }

    #[test]
    fn chinese_triggers() {
        assert_eq!(
            suggest(InputMode::Chinese, "rq"),
            ["2026年11月19日", "2026-11-19", "11月19日 星期四"]
        );
        assert_eq!(suggest(InputMode::Chinese, "sj"), ["21:05", "晚上9點05分"]);
        assert_eq!(suggest(InputMode::Chinese, "xq")[0], "星期四");
    }

    #[test]
    fn english_triggers() {
        assert_eq!(
            suggest(InputMode::English, "date"),
            ["2026-11-19", "November 19, 2026"]
        );
        assert_eq!(suggest(InputMode::English, "Time"), ["21:05"]);
        assert_eq!(suggest(InputMode::English, "weekday"), ["Thursday"]);
    }

    #[test]
    fn other_input_is_ignored() {
        assert!(suggest(InputMode::Chinese, "date").is_empty());
        assert!(suggest(InputMode::English, "rq").is_empty());
        assert!(suggest(InputMode::Chinese, "").is_empty());
    }
}
