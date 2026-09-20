//! Price schedules use IANA timezone rules, including DST and midnight windows.
use crate::api::ApiError;
use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Schedule {
    pub version: u32,
    pub rules: Vec<Rule>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub priority: i32,
    pub timezone: String,
    pub start_minute: u32,
    pub end_minute: u32,
    #[serde(default)]
    pub weekdays: Vec<u32>,
    #[serde(default)]
    pub from: Option<i64>,
    #[serde(default)]
    pub until: Option<i64>,
    pub prices: BTreeMap<String, i64>,
}
impl Schedule {
    pub fn validate(&self) -> Result<(), ApiError> {
        if self.version > 1 || self.rules.len() > 64 {
            return Err(invalid());
        }
        for rule in &self.rules {
            if rule.timezone.parse::<chrono_tz::Tz>().is_err()
                || rule.start_minute >= 1440
                || rule.end_minute > 1440
                || rule.weekdays.iter().any(|n| !(1..=7).contains(n))
                || rule.prices.keys().any(|k| {
                    !matches!(
                        k.as_str(),
                        "input"
                            | "output"
                            | "cache_read"
                            | "cache_write"
                            | "reasoning"
                            | "flat"
                            | "unit"
                    )
                })
                || rule.prices.values().any(|v| *v < 0)
                || rule
                    .from
                    .zip(rule.until)
                    .is_some_and(|(from, until)| from >= until)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub fn prices_at(&self, now: i64) -> Result<Option<&BTreeMap<String, i64>>, ApiError> {
        self.validate()?;
        let timestamp = chrono::DateTime::from_timestamp(now, 0).ok_or_else(invalid)?;
        let mut matched = vec![];
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.from.is_some_and(|from| now < from)
                || rule.until.is_some_and(|until| now >= until)
            {
                continue;
            }
            let local = timestamp.with_timezone(
                &rule
                    .timezone
                    .parse::<chrono_tz::Tz>()
                    .map_err(|_| invalid())?,
            );
            let minute = local.hour() * 60 + local.minute();
            let window = if rule.start_minute == rule.end_minute {
                true
            } else if rule.start_minute < rule.end_minute {
                minute >= rule.start_minute && minute < rule.end_minute
            } else {
                minute >= rule.start_minute || minute < rule.end_minute
            };
            // For a window crossing midnight, its weekday is the day it began.
            let weekday = if rule.start_minute > rule.end_minute && minute < rule.end_minute {
                (local.weekday().number_from_monday() + 5) % 7 + 1
            } else {
                local.weekday().number_from_monday()
            };
            if window && (rule.weekdays.is_empty() || rule.weekdays.contains(&weekday)) {
                matched.push((rule.priority, index, &rule.prices))
            }
        }
        matched.sort_by_key(|(priority, index, _)| (*priority, *index));
        Ok(matched.first().map(|(_, _, prices)| *prices))
    }
}
fn invalid() -> ApiError {
    ApiError::BadRequest("invalid price schedule".into())
}
