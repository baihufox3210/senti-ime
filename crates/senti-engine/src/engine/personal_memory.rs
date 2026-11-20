//! Memory management and retrieval stay outside transient input-context IDs.
use super::*;
use senti_core::{MemoryEntry, MemoryKind, PersonalMemory};

#[derive(PartialEq)]
struct ContextKey {
    scope: String,
    query: String,
    last_id: Option<u64>,
    count: usize,
    retention: u32,
    at: senti_core::LocalDateTime,
}
#[derive(Default)]
pub(super) struct ContextCache(Option<(ContextKey, String)>);

impl InputEngine {
    pub fn memory_scope(&self) -> &str {
        &self.memory.profile
    }
    pub fn memory_config_enabled(&self) -> bool {
        self.memory.enabled
    }
    pub fn memory_enabled(&self) -> bool {
        self.memory.enabled && !self.memory_disabled && self.config.learning_enabled
    }
    pub fn memory_retention_days(&self) -> u32 {
        self.memory.retention_days
    }
    pub fn set_memory_retention_days(&mut self, days: u32) -> Result<(), String> {
        if !(1..=3650).contains(&days) {
            return Err("retention must be 1–3650 days".into());
        }
        self.memory.retention_days = days;
        self.memory.prune(self.ports.clock.now());
        self.persist_memory();
        self.refresh();
        Ok(())
    }
    pub fn memory_persistent(&self) -> bool {
        self.ports.memory_store.is_some() && !self.memory_disabled
    }
    pub fn memory_page(&self, offset: usize, limit: usize) -> Vec<MemoryEntry> {
        self.memory
            .iter(&self.memory.profile, self.ports.clock.now())
            .skip(offset)
            .take(limit.min(512))
            .cloned()
            .collect()
    }
    pub fn memory_entries(&self) -> Vec<MemoryEntry> {
        self.memory
            .list(&self.memory.profile, self.ports.clock.now())
    }
    pub fn set_memory_scope(&mut self, scope: &str) -> Result<(), String> {
        if !PersonalMemory::valid_scope(scope) {
            return Err("invalid memory scope".into());
        }
        if self.memory.profile != scope {
            self.cancel_composition();
            self.last_commit = None;
            self.surrounding.clear();
            self.following.clear();
            self.memory.profile = scope.into();
            self.persist_memory();
            self.refresh();
        }
        Ok(())
    }
    pub fn set_memory_enabled(&mut self, enabled: bool) {
        self.memory.enabled = enabled;
        self.persist_memory();
        self.refresh();
    }
    pub fn remove_memory(&mut self, id: u64) -> bool {
        let scope = self.memory.profile.clone();
        let removed = self.memory.remove(&scope, id);
        if removed {
            self.persist_memory();
            self.refresh();
        }
        removed
    }
    pub fn clear_memory(&mut self) {
        let profile = self.memory.profile.clone();
        self.memory.clear_scope(&profile);
        self.persist_memory();
        self.refresh();
    }
    pub fn flush_memory(&self) -> Result<(), String> {
        if self.memory_disabled {
            return Err("memory persistence is unavailable".into());
        }
        if let Some(store) = &self.ports.memory_store {
            store.flush().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub(super) fn persist_memory(&mut self) {
        if self.memory_disabled {
            return;
        }
        if let Some(store) = &self.ports.memory_store {
            if let Err(error) = store.save(&self.memory) {
                self.last_error = Some(error.to_string());
                self.memory_disabled = true;
            }
        }
    }
    pub(super) fn remember(
        &mut self,
        kind: MemoryKind,
        mode: InputMode,
        code: &str,
        text: &str,
    ) -> bool {
        if !self.memory_enabled() {
            return false;
        }
        let scope = self.memory.profile.clone();
        let recorded = self
            .memory
            .record(&scope, kind, mode, code, text, self.ports.clock.now());
        if recorded && kind == MemoryKind::Completion {
            let context = format!("{}{}", self.surrounding, self.composition.selected_text());
            let chars: Vec<_> = context.chars().collect();
            if let Some(entry) = self.memory.entries.last_mut() {
                entry.context = chars.iter().skip(chars.len().saturating_sub(128)).collect();
            }
        }
        if recorded {
            self.persist_memory();
        }
        recorded
    }
    pub(super) fn remember_correction(
        &mut self,
        mode: InputMode,
        raw: &str,
        previous: &str,
        replacement: &str,
    ) {
        if !self.memory_enabled() || previous == replacement || previous.chars().count() > 256 {
            return;
        }
        if self.remember(
            MemoryKind::Correction,
            mode,
            &code_for(mode, raw),
            replacement,
        ) {
            if let Some(entry) = self.memory.entries.last_mut() {
                entry.replaced = Some(previous.to_owned());
            }
            self.persist_memory();
        }
    }
    pub(super) fn remember_commit(&mut self, text: &str) {
        if !self.memory_enabled()
            || self.memory.profile == "default"
            || self.memory.profile.starts_with("app:")
        {
            return;
        }
        let scope = self.memory.profile.clone();
        self.memory
            .record_commit(&scope, text, self.ports.clock.now());
        self.persist_memory();
    }
    pub(super) fn memory_context(&self, previous: Option<&str>) -> String {
        if !self.memory_enabled()
            || self.memory.profile == "default"
            || self.memory.profile.starts_with("app:")
        {
            return String::new();
        }
        let at = self.ports.clock.now();
        let key = ContextKey {
            scope: self.memory.profile.clone(),
            query: previous.unwrap_or("").into(),
            last_id: self.memory.entries.last().map(|e| e.id),
            count: self.memory.entries.len(),
            retention: self.memory.retention_days,
            at,
        };
        if let Ok(cache) = self.memory_context_cache.lock() {
            if let Some((cached, text)) = &cache.0 {
                if *cached == key {
                    return text.clone();
                }
            }
        }
        let text = self
            .memory
            .context(&self.memory.profile, previous.unwrap_or(""), at);
        if let Ok(mut cache) = self.memory_context_cache.lock() {
            cache.0 = Some((key, text.clone()));
        }
        text
    }
    pub(super) fn memory_candidates(&self, mode: InputMode, input: &str) -> Vec<Candidate> {
        if !self.memory_enabled() || input.is_empty() || self.memory.profile == "default" {
            return Vec::new();
        }
        let code = code_for(mode, input);
        self.memory
            .iter(&self.memory.profile, self.ports.clock.now())
            .filter(|e| {
                e.mode == mode
                    && e.code == code
                    && e.kind != MemoryKind::Commit
                    && (e.kind != MemoryKind::Completion
                        || !e.context.is_empty()
                            && format!("{}{}", self.surrounding, self.composition.selected_text())
                                .ends_with(&e.context))
            })
            .map(|e| {
                Candidate::new(
                    e.text.clone(),
                    input.len(),
                    2.0,
                    if e.kind == MemoryKind::Completion {
                        CandidateSource::Context
                    } else {
                        CandidateSource::UserHistory
                    },
                )
                .with_mode(mode)
                .with_annotation("個人記憶")
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_profiles_never_retrieve_old_conversation_snapshots_and_clear_invalidates_cache() {
        let mut engine = crate::EngineBuilder::new().build();
        engine.memory.profile = "app:editor".into();
        engine.memory.record_commit(
            "app:editor",
            "Atlas server deployment uses Rust.",
            senti_core::LocalDateTime::default(),
        );
        assert!(engine.memory_context(Some("Atlas")).is_empty());
        engine.memory.profile = "work:atlas".into();
        engine.memory.record_commit(
            "work:atlas",
            "Atlas server deployment uses Rust.",
            senti_core::LocalDateTime::default(),
        );
        assert!(!engine.memory_context(Some("Atlas")).is_empty());
        engine.clear_memory();
        assert!(engine.memory_context(Some("Atlas")).is_empty());
    }
}
