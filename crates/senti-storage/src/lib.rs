//! # senti-storage
//!
//! Adapters implementing [`senti_core::UserModelStore`]:
//!
//! * [`JsonFileStore`] – persists the user model as pretty JSON using an
//!   atomic write (temp file + rename) so a crash never leaves a truncated file.
//! * [`InMemoryStore`] – volatile store for tests and "incognito" sessions.

mod json_file;
mod memory;

pub use json_file::JsonFileStore;
pub use memory::InMemoryStore;

mod personal_memory;
pub use personal_memory::JsonMemoryStore;
