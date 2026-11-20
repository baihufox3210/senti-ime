use senti_core::StoreError;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown plugin `{0}`")]
    UnknownPlugin(String),
    #[error("persistence is disabled for this session")]
    PersistenceDisabled,
    #[error(transparent)]
    Store(#[from] StoreError),
}
