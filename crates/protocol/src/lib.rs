//! Versioned binary messages exchanged between the CircuitSim UI and worker.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sim_core::netlist::Netlist;
use sim_core::{AcOptions, AcResult, DcOptions, DcResult, TransientOptions, TransientResult};
use thiserror::Error;

mod netlist_wire;

/// Current wire-format version. Any incompatible layout change must increment it.
pub const PROTOCOL_VERSION: u16 = 4;

/// Four-byte marker at the beginning of every frame.
pub const FRAME_MAGIC: [u8; 4] = *b"CSIM";

/// Size of the fixed frame header in bytes.
pub const HEADER_LEN: usize = 12;

const KIND_REQUEST: u8 = 1;
const KIND_RESPONSE: u8 = 2;
const FLAGS_NONE: u8 = 0;

/// Direction encoded in a frame header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameKind {
    /// A message sent by the UI to the worker.
    Request,
    /// A message sent by the worker to the UI.
    Response,
}

impl FrameKind {
    const fn wire_byte(self) -> u8 {
        match self {
            Self::Request => KIND_REQUEST,
            Self::Response => KIND_RESPONSE,
        }
    }

    const fn from_wire_byte(value: u8) -> Option<Self> {
        match value {
            KIND_REQUEST => Some(Self::Request),
            KIND_RESPONSE => Some(Self::Response),
            _ => None,
        }
    }
}

/// Transport selected by the browser host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportKind {
    /// Portable asynchronous transport based on `postMessage`.
    PostMessage,
    /// Shared-memory transport available only in a cross-origin-isolated page.
    SharedArrayBuffer,
}

/// Selects a transport without assuming that `SharedArrayBuffer` is usable.
pub const fn select_transport(
    shared_array_buffer_available: bool,
    cross_origin_isolated: bool,
) -> TransportKind {
    if shared_array_buffer_available && cross_origin_isolated {
        TransportKind::SharedArrayBuffer
    } else {
        TransportKind::PostMessage
    }
}

/// Frame validation or payload serialization error.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    /// The input does not contain a complete fixed header.
    #[error("frame is shorter than the {expected}-byte header (got {actual} bytes)")]
    HeaderTooShort {
        /// Minimum number of bytes required.
        expected: usize,
        /// Number of bytes actually supplied.
        actual: usize,
    },
    /// The four-byte marker is not `CSIM`.
    #[error("invalid frame magic")]
    InvalidMagic,
    /// The frame was produced by an incompatible protocol version.
    #[error("unsupported protocol version {received}; this build supports {supported}")]
    UnsupportedVersion {
        /// Version found in the frame.
        received: u16,
        /// Version supported by this build.
        supported: u16,
    },
    /// The frame direction byte is not recognized.
    #[error("unknown frame kind {kind}")]
    UnknownFrameKind {
        /// Unrecognized direction byte.
        kind: u8,
    },
    /// Reserved flag bits are nonzero.
    #[error("unsupported frame flags 0x{flags:02x}")]
    UnsupportedFlags {
        /// Reserved flag byte found in the frame.
        flags: u8,
    },
    /// A request decoder received a response, or the reverse.
    #[error("expected a {expected:?} frame, got {actual:?}")]
    UnexpectedFrameKind {
        /// Direction required by the decoder.
        expected: FrameKind,
        /// Direction found in the frame.
        actual: FrameKind,
    },
    /// The declared payload is shorter than the bytes required by the header.
    #[error("declared payload length {declared} exceeds the {available} available bytes")]
    TruncatedPayload {
        /// Payload length declared by the header.
        declared: u32,
        /// Number of payload bytes actually available.
        available: usize,
    },
    /// Bytes follow the declared payload.
    #[error("frame declares {declared} payload bytes but {actual} follow the header")]
    TrailingBytes {
        /// Payload length declared by the header.
        declared: u32,
        /// Number of payload bytes actually available.
        actual: usize,
    },
    /// A payload cannot be represented by the 32-bit frame length field.
    #[error("frame payload exceeds the 32-bit length field ({length} bytes)")]
    PayloadTooLarge {
        /// Unencoded payload length.
        length: usize,
    },
    /// Serde could not encode the payload.
    #[error("could not encode frame payload: {message}")]
    Encode {
        /// Backend error description.
        message: String,
    },
    /// Serde could not decode the payload.
    #[error("could not decode frame payload: {message}")]
    Decode {
        /// Backend error description.
        message: String,
    },
}

/// Message sent by the UI to the worker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Requests a simulation and replaces any older pending job.
    Submit {
        /// Monotonic revision used to discard stale replies.
        job: Job,
    },
    /// Checks that the worker and frame codec are alive.
    Ping {
        /// Opaque value echoed by the worker.
        nonce: u64,
    },
}

/// Complete simulation request carried by one frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// Strictly increasing request revision.
    pub revision: u64,
    /// Validated netlist snapshot used by the analysis.
    #[serde(with = "netlist_wire")]
    pub netlist: Netlist,
    /// Analysis and its options.
    pub analysis: Analysis,
}

/// Analysis selector with all options required by the solver.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Analysis {
    /// DC operating point.
    Dc(DcOptions),
    /// Fixed-step transient analysis.
    Transient(TransientOptions),
    /// Small-signal AC sweep.
    Ac(AcOptions),
}

impl Analysis {
    /// Returns the analysis family without borrowing its options.
    pub const fn kind(&self) -> AnalysisKind {
        match self {
            Self::Dc(_) => AnalysisKind::Dc,
            Self::Transient(_) => AnalysisKind::Transient,
            Self::Ac(_) => AnalysisKind::Ac,
        }
    }
}

/// Stable analysis family tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnalysisKind {
    /// DC operating point.
    Dc,
    /// Fixed-step transient analysis.
    Transient,
    /// Small-signal AC sweep.
    Ac,
}

/// Successful analysis result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AnalysisResult {
    /// DC operating point and branch currents.
    Dc(DcResult),
    /// Transient samples and energy audit.
    Transient(TransientResult),
    /// AC operating point and frequency responses.
    Ac(AcResult),
}

impl AnalysisResult {
    /// Returns the analysis family of the result.
    pub const fn kind(&self) -> AnalysisKind {
        match self {
            Self::Dc(_) => AnalysisKind::Dc,
            Self::Transient(_) => AnalysisKind::Transient,
            Self::Ac(_) => AnalysisKind::Ac,
        }
    }
}

/// Message sent by the worker to the UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// The job was stored in the single pending slot.
    Accepted {
        /// Revision stored by the worker.
        revision: u64,
    },
    /// The job replaced an older pending job.
    Coalesced {
        /// Revision now stored by the worker.
        revision: u64,
        /// Older revision discarded without execution.
        replaced: u64,
    },
    /// The submission was refused.
    Rejected {
        /// Refused revision.
        revision: u64,
        /// Reason for the refusal.
        reason: RejectionReason,
    },
    /// The job completed successfully.
    Result {
        /// Revision that produced this result.
        revision: u64,
        /// Complete analysis output.
        result: AnalysisResult,
    },
    /// The job failed during validation or simulation.
    Failed {
        /// Revision that produced this failure.
        revision: u64,
        /// Typed failure stage and diagnostic text.
        failure: AnalysisFailure,
    },
    /// Reply to a ping.
    Pong {
        /// Value copied from the corresponding request.
        nonce: u64,
    },
}

/// Reason a job was not placed in the worker slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RejectionReason {
    /// Revision zero is reserved and never valid.
    InvalidRevision,
    /// Revision is not strictly greater than the latest accepted revision.
    StaleRevision {
        /// Greatest revision accepted by this worker session.
        latest: u64,
    },
}

/// Analysis stage that produced a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnalysisFailureKind {
    /// Netlist boundary validation failed before an analysis started.
    Netlist,
    /// DC solver failure.
    Dc,
    /// Transient solver failure.
    Transient,
    /// AC solver failure.
    Ac,
}

/// Typed failure stage with a human-readable diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisFailure {
    /// Stage that rejected the job.
    pub kind: AnalysisFailureKind,
    /// Stable API error rendered as text for display and logging.
    pub message: String,
}

/// Encodes a request into a framed byte vector.
pub fn encode_request(message: &ClientMessage) -> Result<Vec<u8>, ProtocolError> {
    encode_frame(FrameKind::Request, message)
}

/// Decodes one complete request frame.
pub fn decode_request(frame: &[u8]) -> Result<ClientMessage, ProtocolError> {
    decode_frame(frame, FrameKind::Request)
}

/// Encodes a response into a framed byte vector.
pub fn encode_response(message: &ServerMessage) -> Result<Vec<u8>, ProtocolError> {
    encode_frame(FrameKind::Response, message)
}

/// Decodes one complete response frame.
pub fn decode_response(frame: &[u8]) -> Result<ServerMessage, ProtocolError> {
    decode_frame(frame, FrameKind::Response)
}

fn encode_frame<T: Serialize>(kind: FrameKind, message: &T) -> Result<Vec<u8>, ProtocolError> {
    let payload = postcard::to_allocvec(message).map_err(|error| ProtocolError::Encode {
        message: error.to_string(),
    })?;
    let payload_length =
        u32::try_from(payload.len()).map_err(|_| ProtocolError::PayloadTooLarge {
            length: payload.len(),
        })?;
    let mut frame = Vec::with_capacity(HEADER_LEN + payload.len());
    frame.extend_from_slice(&FRAME_MAGIC);
    frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    frame.push(kind.wire_byte());
    frame.push(FLAGS_NONE);
    frame.extend_from_slice(&payload_length.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn decode_frame<T: DeserializeOwned>(
    frame: &[u8],
    expected_kind: FrameKind,
) -> Result<T, ProtocolError> {
    let (kind, payload) = split_frame(frame)?;
    if kind != expected_kind {
        return Err(ProtocolError::UnexpectedFrameKind {
            expected: expected_kind,
            actual: kind,
        });
    }
    postcard::from_bytes(payload).map_err(|error| ProtocolError::Decode {
        message: error.to_string(),
    })
}

fn split_frame(frame: &[u8]) -> Result<(FrameKind, &[u8]), ProtocolError> {
    if frame.len() < HEADER_LEN {
        return Err(ProtocolError::HeaderTooShort {
            expected: HEADER_LEN,
            actual: frame.len(),
        });
    }
    if frame[0..4] != FRAME_MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }
    let version = u16::from_le_bytes([frame[4], frame[5]]);
    if version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion {
            received: version,
            supported: PROTOCOL_VERSION,
        });
    }
    let kind = FrameKind::from_wire_byte(frame[6])
        .ok_or(ProtocolError::UnknownFrameKind { kind: frame[6] })?;
    if frame[7] != FLAGS_NONE {
        return Err(ProtocolError::UnsupportedFlags { flags: frame[7] });
    }
    let declared = u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]);
    let available = frame.len() - HEADER_LEN;
    let declared_length = declared as usize;
    if available < declared_length {
        return Err(ProtocolError::TruncatedPayload {
            declared,
            available,
        });
    }
    if available > declared_length {
        return Err(ProtocolError::TrailingBytes {
            declared,
            actual: available,
        });
    }
    Ok((kind, &frame[HEADER_LEN..]))
}
