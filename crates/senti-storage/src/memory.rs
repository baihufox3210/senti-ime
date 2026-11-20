use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use senti_core::{StoreError, UserModel, UserModelStore};

/// Keeps the user model in memory only.
#[derive(Debug, Default)]
pub struct InMemoryStore {
    model: Mutex<Option<UserModel>>,
    saves: AtomicUsize,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start with a pre-populated model.
    pub fn with_model(model: UserModel) -> Self {
        Self {
            model: Mutex::new(Some(model)),
            saves: AtomicUsize::new(0),
        }
    }

    /// Number of successful `save` calls (useful in tests).
    pub fn save_count(&self) -> usize {
        self.saves.load(Ordering::Relaxed)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<UserModel>> {
        // A poisoned lock only means another thread panicked mid-save; the
        // stored value is still a complete model, so keep using it.
        self.model
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

impl UserModelStore for InMemoryStore {
    fn load(&self) -> Result<UserModel, StoreError> {
        Ok(self.lock().clone().unwrap_or_default())
    }

    fn save(&self, model: &UserModel) -> Result<(), StoreError> {
        *self.lock() = Some(model.clone());
        self.saves.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use senti_core::InputMode;

    #[test]
    fn round_trips_and_counts_saves() {
        let store = InMemoryStore::new();
        assert!(store.load().unwrap().is_empty());
        let mut model = UserModel::default();
        model.record_word(InputMode::English, "hello", "hello");
        store.save(&model).unwrap();
        assert_eq!(store.load().unwrap(), model);
        assert_eq!(store.save_count(), 1);
    }
}
