/// Errors raised by [`crate::UserModelStore`] adapters.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid user model data: {0}")]
    Format(String),
}
