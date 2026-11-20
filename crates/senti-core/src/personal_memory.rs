//! Bounded, scoped personal memory. Automatic commits are context, not votes.
use crate::{InputMode, LocalDateTime, StoreError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Commit,
    Confirmed,
    Completion,
    Correction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEntry {
    #[serde(default = "one")]
    pub accepts: u32,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub replaced: Option<String>,
    pub id: u64,
    pub scope: String,
    pub kind: MemoryKind,
    pub mode: InputMode,
    pub code: String,
    pub text: String,
    pub at: LocalDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonalMemory {
    pub version: u32,
    pub enabled: bool,
    pub profile: String,
    pub retention_days: u32,
    pub entries: Vec<MemoryEntry>,
    next_id: u64,
}
impl Default for PersonalMemory {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: true,
            profile: "default".into(),
            retention_days: 30,
            entries: Vec::new(),
            next_id: 1,
        }
    }
}
pub trait MemoryStore: Send {
    fn load(&self) -> Result<PersonalMemory, StoreError>;
    fn save(&self, memory: &PersonalMemory) -> Result<(), StoreError>;
    fn flush(&self) -> Result<(), StoreError> {
        Ok(())
    }
}

fn one() -> u32 {
    1
}

impl PersonalMemory {
    pub fn valid_scope(scope: &str) -> bool {
        !scope.trim().is_empty() && scope.len() <= 160 && !scope.chars().any(char::is_control)
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        if self.version != 1
            || !Self::valid_scope(&self.profile)
            || !(1..=3650).contains(&self.retention_days)
            || self.entries.len() > 512
        {
            return Err(StoreError::Format(
                "unsupported or invalid personal memory".into(),
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for e in &self.entries {
            if !Self::valid_scope(&e.scope)
                || e.text.chars().count() > 256
                || e.context.chars().count() > 128
                || e.replaced.as_ref().is_some_and(|s| s.chars().count() > 256)
                || e.code.len() > 1024
                || e.code.chars().any(char::is_control)
                || e.id == 0
                || !seen.insert(e.id)
                || e.id >= self.next_id
            {
                return Err(StoreError::Format("invalid personal memory entry".into()));
            }
        }
        Ok(())
    }
    pub fn prune(&mut self, now: LocalDateTime) {
        let oldest = minute(now) - i64::from(self.retention_days) * 1440;
        self.entries.retain(|e| minute(e.at) >= oldest);
    }
    pub fn record(
        &mut self,
        scope: &str,
        kind: MemoryKind,
        mode: InputMode,
        code: &str,
        text: &str,
        now: LocalDateTime,
    ) -> bool {
        if !self.enabled
            || !Self::valid_scope(scope)
            || text.trim().is_empty()
            || code.len() > 1024
            || code.chars().any(char::is_control)
            || text.chars().count() > 256
        {
            return false;
        }
        self.prune(now);
        let accepts = self
            .entries
            .iter()
            .find(|e| {
                e.scope == scope
                    && e.kind == kind
                    && e.mode == mode
                    && e.code == code
                    && e.text == text
            })
            .map_or(1, |e| e.accepts.saturating_add(1));
        self.entries.retain(|e| {
            !(e.scope == scope
                && e.kind == kind
                && e.mode == mode
                && e.code == code
                && e.text == text)
        });
        if self.entries.len() >= 512 {
            let remove = self
                .entries
                .iter()
                .position(|e| e.kind == MemoryKind::Commit)
                .unwrap_or(0);
            self.entries.remove(remove);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push(MemoryEntry {
            accepts,
            context: String::new(),
            replaced: None,
            id,
            scope: scope.into(),
            kind,
            mode,
            code: code.into(),
            text: text.into(),
            at: now,
        });
        true
    }
    pub fn record_commit(&mut self, scope: &str, text: &str, now: LocalDateTime) {
        if !self.enabled {
            return;
        }
        let previous = self
            .entries
            .iter()
            .rev()
            .find(|e| e.scope == scope && e.kind == MemoryKind::Commit);
        let mut combined = text.to_owned();
        let mut replace = None;
        if let Some(e) = previous.filter(|e| {
            (0..=10).contains(&(minute(now) - minute(e.at)))
                && !e.text.ends_with(['。', '！', '？', '.', '!', '?', '\n'])
                && e.text.chars().count() + text.chars().count() <= 256
        }) {
            combined = format!("{}{}", e.text, text);
            replace = Some(e.id);
        }
        let chars: Vec<_> = combined.chars().collect();
        let combined: String = chars.iter().skip(chars.len().saturating_sub(256)).collect();
        if let Some(id) = replace {
            self.entries.retain(|e| e.id != id);
        }
        // Keep older sessions too, but bound context independently of vocabulary.
        if self
            .entries
            .iter()
            .filter(|e| e.kind == MemoryKind::Commit)
            .count()
            >= 128
        {
            if let Some(old) = self
                .entries
                .iter()
                .position(|e| e.kind == MemoryKind::Commit)
            {
                self.entries.remove(old);
            }
        }
        self.record(
            scope,
            MemoryKind::Commit,
            InputMode::Mixed,
            "",
            &combined,
            now,
        );
    }
    pub fn iter<'a>(
        &'a self,
        scope: &'a str,
        now: LocalDateTime,
    ) -> impl Iterator<Item = &'a MemoryEntry> {
        let oldest = minute(now) - i64::from(self.retention_days) * 1440;
        self.entries
            .iter()
            .rev()
            .filter(move |e| e.scope == scope && minute(e.at) >= oldest)
    }
    pub fn list(&self, scope: &str, now: LocalDateTime) -> Vec<MemoryEntry> {
        self.iter(scope, now).cloned().collect()
    }
    pub fn remove(&mut self, scope: &str, id: u64) -> bool {
        let old = self.entries.len();
        self.entries.retain(|e| e.scope != scope || e.id != id);
        old != self.entries.len()
    }
    pub fn clear_scope(&mut self, scope: &str) {
        self.entries.retain(|e| e.scope != scope);
    }
    /// Retrieve only an explicitly shared topic, never an unrelated old thread.
    pub fn context(&self, scope: &str, query: &str, now: LocalDateTime) -> String {
        if !self.enabled || query.trim().is_empty() {
            return String::new();
        }
        let tail: String = query
            .chars()
            .rev()
            .take(80)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let terms = topic_terms(&tail);
        let mut matches: Vec<_> = self
            .iter(scope, now)
            .filter(|e| e.kind == MemoryKind::Commit && e.text.chars().count() >= 8)
            .filter_map(|e| {
                let lower = e.text.to_lowercase();
                let score = terms.iter().filter(|t| lower.contains(t.as_str())).count();
                (score >= 2
                    || terms
                        .iter()
                        .any(|t| t.len() >= 5 && t.is_ascii() && lower.contains(t.as_str())))
                .then_some((score, e))
            })
            .collect();
        matches.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.id.cmp(&a.1.id)));
        matches
            .into_iter()
            .take(2)
            .map(|(_, e)| e.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
fn topic_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut latin = String::new();
    let mut previous = None;
    for c in text.to_lowercase().chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() {
            latin.push(c);
            previous = None;
            continue;
        }
        if latin.len() >= 4 {
            terms.push(std::mem::take(&mut latin));
        } else {
            latin.clear();
        }
        if ('\u{3400}'..='\u{9fff}').contains(&c) {
            if let Some(p) = previous {
                terms.push(format!("{p}{c}"));
            }
            previous = Some(c);
        } else {
            previous = None;
        }
    }
    terms.sort();
    terms.dedup();
    terms
}
fn minute(t: LocalDateTime) -> i64 {
    let y = i64::from(t.year) - i64::from(t.month <= 2);
    let era = y.div_euclid(400);
    let year = y - era * 400;
    let month = i64::from(t.month) + if t.month > 2 { -3 } else { 9 };
    let days = era * 146097 + year * 365 + year / 4 - year / 100
        + (153 * month + 2) / 5
        + i64::from(t.day)
        - 1;
    days * 1440 + i64::from(t.hour) * 60 + i64::from(t.minute)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn time(day: u32) -> LocalDateTime {
        LocalDateTime {
            year: 2026,
            month: 11,
            day,
            ..LocalDateTime::default()
        }
    }
    #[test]
    fn scoped_memory_expires_and_does_not_turn_automatic_text_into_preferences() {
        let mut m = PersonalMemory::default();
        m.record(
            "work",
            MemoryKind::Confirmed,
            InputMode::Zhuyin,
            "su3",
            "妳",
            time(1),
        );
        m.record_commit("work", "Atlas 伺服器部署使用 Rust。", time(1));
        assert_eq!(m.list("work", time(2)).len(), 2);
        assert!(m.list("private", time(2)).is_empty());
        assert!(!m.context("work", "Atlas", time(2)).is_empty());
        assert!(m.context("private", "Atlas", time(2)).is_empty());
        assert!(m.context("work", "早餐", time(2)).is_empty());
        m.retention_days = 1;
        m.prune(time(4));
        assert!(m.entries.is_empty());
    }
    #[test]
    fn bounded_snapshot_has_unique_ids_and_counts_only_repeated_acceptance() {
        let mut m = PersonalMemory::default();
        for i in 0..600 {
            m.record(
                "work",
                MemoryKind::Confirmed,
                InputMode::English,
                "term",
                &format!("term{i}"),
                time(1),
            );
        }
        assert_eq!(m.entries.len(), 512);
        m.validate().unwrap();
        m.record(
            "work",
            MemoryKind::Confirmed,
            InputMode::English,
            "term",
            "term599",
            time(2),
        );
        assert_eq!(m.entries.last().unwrap().accepts, 2);
        let id = m.entries.last().unwrap().id;
        assert!(!m.remove("private", id));
        assert!(m.remove("work", id));
        m.clear_scope("work");
        assert!(m.entries.is_empty());
        m.enabled = false;
        m.record_commit("work", "do not remember", time(2));
        assert!(m.entries.is_empty());
    }
}
