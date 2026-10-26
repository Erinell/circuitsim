//! Frame-oriented worker adapter shared by native tests and WASM.

use thiserror::Error;

use worker_protocol::{
    decode_request, encode_response, ClientMessage, ProtocolError, ServerMessage,
};

use crate::session::{SubmitOutcome, WorkerConfig, WorkerError, WorkerSession};

/// Error raised while handling one worker frame.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// The input frame is malformed or has the wrong direction/version.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// The worker clock or scheduling configuration is invalid.
    #[error(transparent)]
    Worker(#[from] WorkerError),
}

/// Encodes and decodes frames around a [`WorkerSession`].
pub struct WorkerService {
    session: WorkerSession,
}

impl WorkerService {
    /// Creates a frame service with validated worker configuration.
    pub fn new(config: WorkerConfig) -> Result<Self, WorkerError> {
        Ok(Self {
            session: WorkerSession::new(config)?,
        })
    }

    /// Handles one request and returns its immediate acknowledgement or pong.
    ///
    /// Analysis results are not returned here; the host calls [`Self::poll_frame`].
    pub fn submit_frame(&mut self, frame: &[u8]) -> Result<Vec<u8>, ServiceError> {
        let response = match decode_request(frame)? {
            ClientMessage::Submit { job } => acknowledge(self.session.submit(job)),
            ClientMessage::Ping { nonce } => ServerMessage::Pong { nonce },
        };
        Ok(encode_response(&response)?)
    }

    /// Runs the pending analysis when allowed and returns one encoded response.
    pub fn poll_frame(&mut self, now_seconds: f64) -> Result<Option<Vec<u8>>, ServiceError> {
        match self.session.poll(now_seconds)? {
            Some(response) => Ok(Some(encode_response(&response)?)),
            None => Ok(None),
        }
    }

    /// Returns the pending revision, if any.
    pub fn pending_revision(&self) -> Option<u64> {
        self.session.pending_revision()
    }

    /// Returns true when an analysis is waiting for its scheduled slot.
    pub fn has_pending(&self) -> bool {
        self.session.has_pending()
    }
}

fn acknowledge(outcome: SubmitOutcome) -> ServerMessage {
    match outcome {
        SubmitOutcome::Accepted { revision } => ServerMessage::Accepted { revision },
        SubmitOutcome::Coalesced { revision, replaced } => {
            ServerMessage::Coalesced { revision, replaced }
        }
        SubmitOutcome::Rejected { revision, reason } => {
            ServerMessage::Rejected { revision, reason }
        }
    }
}
