//! `wasm-bindgen` bridge for a dedicated browser worker.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;

use crate::{WorkerConfig, WorkerService};

/// Browser-side facade around the frame-oriented worker service.
#[wasm_bindgen(js_name = "CircuitWorker")]
pub struct CircuitWorker {
    service: WorkerService,
}

#[wasm_bindgen]
impl CircuitWorker {
    /// Creates a worker with the default 60 Hz analysis gate.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Self, JsValue> {
        WorkerService::new(WorkerConfig::default())
            .map(|service| Self { service })
            .map_err(js_error)
    }

    /// Handles one request frame and returns its acknowledgement or pong.
    #[wasm_bindgen(js_name = "submitFrame")]
    pub fn submit_frame(&mut self, frame: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.service.submit_frame(frame).map_err(js_error)
    }

    /// Runs at most one pending analysis and returns one response frame.
    #[wasm_bindgen(js_name = "pollFrame")]
    pub fn poll_frame(&mut self, now_seconds: f64) -> Result<Option<Vec<u8>>, JsValue> {
        self.service.poll_frame(now_seconds).map_err(js_error)
    }

    /// Returns true while a submitted job is waiting for its scheduled slot.
    #[wasm_bindgen(js_name = "hasPending")]
    pub fn has_pending(&self) -> bool {
        self.service.has_pending()
    }
}

/// Returns the wire protocol version implemented by this WASM module.
#[wasm_bindgen(js_name = "protocolVersion")]
pub fn protocol_version() -> u16 {
    worker_protocol::PROTOCOL_VERSION
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
