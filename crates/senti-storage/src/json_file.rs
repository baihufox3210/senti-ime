use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use senti_core::{StoreError, UserModel, UserModelStore};

/// Stores the user model in a single JSON file.
#[derive(Debug, Clone)]
pub struct JsonFileStore {
    path: PathBuf,
}

impl JsonFileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn temp_path(&self) -> PathBuf {
        let mut name = self
            .path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| "user_model.json".into());
        name.push(".tmp");
        self.path.with_file_name(name)
    }
}

impl UserModelStore for JsonFileStore {
    fn load(&self) -> Result<UserModel, StoreError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(UserModel::default()),
            Err(err) => return Err(err.into()),
        };
        serde_json::from_slice(&bytes).map_err(|err| StoreError::Format(err.to_string()))
    }

    fn save(&self, model: &UserModel) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let json =
            serde_json::to_vec_pretty(model).map_err(|err| StoreError::Format(err.to_string()))?;
        let temp = self.temp_path();
        {
            let mut file = fs::File::create(&temp)?;
            file.write_all(&json)?;
            file.sync_all()?;
        }
        fs::rename(&temp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use senti_core::InputMode;

    #[test]
    fn missing_file_loads_empty_model() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonFileStore::new(dir.path().join("absent.json"));
        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn save_then_load_round_trips_and_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonFileStore::new(dir.path().join("nested/dir/model.json"));
        let mut model = UserModel::default();
        model.record_word(InputMode::Chinese, "nihao", "你好");
        store.save(&model).unwrap();
        assert_eq!(store.load().unwrap(), model);
        assert!(
            !store.temp_path().exists(),
            "temp file must be renamed away"
        );
    }

    #[test]
    fn corrupt_file_is_reported_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.json");
        fs::write(&path, b"{ not json").unwrap();
        let store = JsonFileStore::new(&path);
        assert!(matches!(store.load(), Err(StoreError::Format(_))));
        assert_eq!(fs::read(&path).unwrap(), b"{ not json");
    }
}
