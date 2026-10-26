//! CircuitSim worker runtime, frame adapter, and browser WASM entry point.

mod execute;
mod service;
mod session;

/// The binary protocol shared with the interface.
pub use worker_protocol as protocol;

pub use service::{ServiceError, WorkerService};
pub use session::{SubmitOutcome, WorkerConfig, WorkerError, WorkerSession, DEFAULT_TARGET_HZ};

#[cfg(target_arch = "wasm32")]
mod wasm;
