//! Configuration, migration history, and cache persistence.

mod json_store;

pub use json_store::Storage;
#[cfg(test)]
pub use json_store::StoragePaths;
