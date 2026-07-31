//! Long-running background task coordination.

mod registry;

pub use registry::{CancellationFlag, TaskRegistry};
