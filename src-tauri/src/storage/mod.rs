//! Configuration, migration history, and cache persistence.

mod json_store;
mod process_lock;

pub use json_store::Storage;
#[cfg(test)]
pub use json_store::StoragePaths;
pub use process_lock::ProcessFileLock;
