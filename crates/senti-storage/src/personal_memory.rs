//! Coalesced memory snapshots are written off the input thread, atomically.
use senti_core::{MemoryStore, PersonalMemory, StoreError};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread,
};
#[derive(Default)]
struct Pending {
    snapshot: Option<PersonalMemory>,
    revision: u64,
    saved: u64,
    shutdown: bool,
    error: Option<String>,
}
pub struct JsonMemoryStore {
    path: PathBuf,
    shared: Arc<(Mutex<Pending>, Condvar)>,
    worker: Option<thread::JoinHandle<()>>,
}
impl std::fmt::Debug for JsonMemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonMemoryStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}
impl JsonMemoryStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let state = Arc::clone(&shared);
        let output = path.clone();
        let worker = thread::spawn(move || {
            loop {
                let (lock, signal) = &*state;
                let mut pending = lock.lock().unwrap_or_else(|e| e.into_inner());
                while pending.snapshot.is_none() && !pending.shutdown {
                    pending = signal.wait(pending).unwrap_or_else(|e| e.into_inner());
                }
                let Some(snapshot) = pending.snapshot.take() else {
                    break;
                };
                let revision = pending.revision;
                drop(pending);
                let result = write(&output, &snapshot);
                let mut pending = lock.lock().unwrap_or_else(|e| e.into_inner());
                pending.saved = revision;
                pending.error = result.err().map(|e| e.to_string());
                signal.notify_all();
            }
        });
        Self {
            path,
            shared,
            worker: Some(worker),
        }
    }
}
fn write(path: &Path, memory: &PersonalMemory) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(&serde_json::to_vec(memory).map_err(|e| StoreError::Format(e.to_string()))?)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    Ok(())
}
impl MemoryStore for JsonMemoryStore {
    fn load(&self) -> Result<PersonalMemory, StoreError> {
        let file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(PersonalMemory::default());
            }
            Err(e) => return Err(e.into()),
        };
        let mut bytes = Vec::new();
        file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(StoreError::Format(
                "personal memory file exceeds 4 MiB".into(),
            ));
        }
        let memory: PersonalMemory =
            serde_json::from_slice(&bytes).map_err(|e| StoreError::Format(e.to_string()))?;
        memory.validate()?;
        Ok(memory)
    }
    fn save(&self, memory: &PersonalMemory) -> Result<(), StoreError> {
        memory.validate()?;
        let (lock, signal) = &*self.shared;
        let mut pending = lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(error) = &pending.error {
            return Err(StoreError::Format(error.clone()));
        }
        pending.revision += 1;
        pending.snapshot = Some(memory.clone());
        signal.notify_one();
        Ok(())
    }
    fn flush(&self) -> Result<(), StoreError> {
        let (lock, signal) = &*self.shared;
        let mut pending = lock.lock().unwrap_or_else(|e| e.into_inner());
        let target = pending.revision;
        while pending.saved < target {
            pending = signal.wait(pending).unwrap_or_else(|e| e.into_inner());
        }
        if let Some(error) = &pending.error {
            Err(StoreError::Format(error.clone()))
        } else {
            Ok(())
        }
    }
}
impl Drop for JsonMemoryStore {
    fn drop(&mut self) {
        let (lock, signal) = &*self.shared;
        lock.lock().unwrap_or_else(|e| e.into_inner()).shutdown = true;
        signal.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use senti_core::{InputMode, LocalDateTime, MemoryKind};
    #[test]
    fn coalesced_updates_flush_and_survive_reload_without_touching_legacy_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.json");
        let legacy = dir.path().join("user_model.json");
        fs::write(&legacy, b"legacy bytes").unwrap();
        let mut memory = PersonalMemory::default();
        {
            let store = JsonMemoryStore::new(&path);
            for i in 0..40 {
                memory.record(
                    "work",
                    MemoryKind::Confirmed,
                    InputMode::English,
                    "word",
                    &format!("word{i}"),
                    LocalDateTime::default(),
                );
                store.save(&memory).unwrap();
            }
            store.flush().unwrap();
        }
        let store = JsonMemoryStore::new(&path);
        assert_eq!(store.load().unwrap(), memory);
        assert_eq!(fs::read(legacy).unwrap(), b"legacy bytes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn corrupt_memory_is_reported_and_background_write_failures_are_observable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.json");
        fs::write(&path, b"broken").unwrap();
        let store = JsonMemoryStore::new(&path);
        assert!(store.load().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken");
        drop(store);
        let store = JsonMemoryStore::new(dir.path());
        store.save(&PersonalMemory::default()).unwrap();
        assert!(store.flush().is_err());
    }
}
