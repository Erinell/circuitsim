use std::collections::BTreeMap;

use sim_core::netlist::{ComponentKind, Netlist, NetlistBuilder, NodeKind, ParamMap, ParamValue};
use sim_core::{DcOptions, DcResult};
use worker_protocol::{
    decode_request, decode_response, encode_request, encode_response, select_transport, Analysis,
    AnalysisFailure, AnalysisFailureKind, AnalysisResult, ClientMessage, FrameKind, Job,
    ProtocolError, RejectionReason, ServerMessage, TransportKind, FRAME_MAGIC, HEADER_LEN,
    PROTOCOL_VERSION,
};

fn number(key: &str, value: f64) -> ParamMap {
    let mut params = ParamMap::new();
    params.insert(key.to_owned(), ParamValue::Number(value));
    params
}

fn netlist() -> Netlist {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    builder
        .add_component(
            ComponentKind::VoltageSource,
            "V1",
            number("dc", 5.0),
            vec![output, ground],
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::Resistor,
            "R1",
            number("resistance", 1_000.0),
            vec![output, ground],
        )
        .unwrap();
    builder.build().unwrap()
}

fn request(revision: u64) -> ClientMessage {
    ClientMessage::Submit {
        job: Job {
            revision,
            netlist: netlist(),
            analysis: Analysis::Dc(DcOptions::default()),
        },
    }
}

#[test]
fn request_frame_has_the_documented_header_and_roundtrips() {
    let message = request(7);
    let frame = encode_request(&message).unwrap();

    assert_eq!(&frame[0..4], &FRAME_MAGIC);
    assert_eq!(u16::from_le_bytes([frame[4], frame[5]]), PROTOCOL_VERSION);
    assert_eq!(frame[6], 1);
    assert_eq!(frame[7], 0);
    assert_eq!(HEADER_LEN, 12);
    let payload_length = u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]) as usize;
    assert_eq!(payload_length, frame.len() - HEADER_LEN);
    assert_eq!(decode_request(&frame).unwrap(), message);
    assert_eq!(encode_request(&message).unwrap(), frame);
}

#[test]
fn netlist_wire_roundtrips_parameter_variants_and_custom_kinds() {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let mut switch_params = ParamMap::new();
    switch_params.insert("ron".to_owned(), ParamValue::Number(1.0));
    switch_params.insert("roff".to_owned(), ParamValue::Number(1e9));
    switch_params.insert("closed".to_owned(), ParamValue::Bool(true));
    builder
        .add_component(
            ComponentKind::Switch,
            "SW1",
            switch_params,
            vec![output, ground],
        )
        .unwrap();
    let mut custom_params = ParamMap::new();
    custom_params.insert(
        "model".to_owned(),
        ParamValue::Text("thermistor".to_owned()),
    );
    let sensor = builder
        .add_component(
            ComponentKind::Custom("sensor".to_owned()),
            "TH1",
            custom_params,
            vec![output, ground],
        )
        .unwrap();
    builder
        .add_node(
            "internal",
            NodeKind::Internal {
                owner: sensor,
                index: 0,
            },
        )
        .unwrap();
    builder
        .add_component(
            ComponentKind::ShuntReference,
            "U1",
            [
                ("vref".to_owned(), ParamValue::Number(2.495)),
                ("iref".to_owned(), ParamValue::Number(2e-6)),
                ("ik_min".to_owned(), ParamValue::Number(1e-3)),
                ("imax".to_owned(), ParamValue::Number(0.1)),
                ("z_ka".to_owned(), ParamValue::Number(0.2)),
            ]
            .into(),
            vec![output, output, ground],
        )
        .unwrap();
    let message = ClientMessage::Submit {
        job: Job {
            revision: 1,
            netlist: builder.build().unwrap(),
            analysis: Analysis::Dc(DcOptions::default()),
        },
    };

    let decoded = decode_request(&encode_request(&message).unwrap()).unwrap();

    assert_eq!(decoded, message);
}

#[test]
fn ping_and_response_messages_roundtrip() {
    let ping = ClientMessage::Ping { nonce: 42 };
    assert_eq!(
        decode_request(&encode_request(&ping).unwrap()).unwrap(),
        ping
    );

    let dc = DcResult {
        node_voltages: BTreeMap::new(),
        branch_currents: BTreeMap::new(),
        iterations: 0,
        residual: 0.0,
    };
    let response = ServerMessage::Result {
        revision: 7,
        result: AnalysisResult::Dc(dc),
    };
    assert_eq!(
        decode_response(&encode_response(&response).unwrap()).unwrap(),
        response
    );
}

#[test]
fn failure_and_rejection_messages_roundtrip() {
    let failure = ServerMessage::Failed {
        revision: 8,
        failure: AnalysisFailure {
            kind: AnalysisFailureKind::Transient,
            message: "invalid transient options".to_owned(),
        },
    };
    assert_eq!(
        decode_response(&encode_response(&failure).unwrap()).unwrap(),
        failure
    );

    let rejection = ServerMessage::Rejected {
        revision: 3,
        reason: RejectionReason::StaleRevision { latest: 9 },
    };
    assert_eq!(
        decode_response(&encode_response(&rejection).unwrap()).unwrap(),
        rejection
    );
}

#[test]
fn rejects_a_short_or_malformed_header() {
    assert_eq!(
        decode_request(b"CSI").unwrap_err(),
        ProtocolError::HeaderTooShort {
            expected: HEADER_LEN,
            actual: 3
        }
    );

    let mut frame = encode_request(&request(1)).unwrap();
    frame[0] = b'X';
    assert_eq!(
        decode_request(&frame).unwrap_err(),
        ProtocolError::InvalidMagic
    );
}

#[test]
fn rejects_an_unsupported_version_before_decoding_the_payload() {
    let mut frame = encode_request(&request(1)).unwrap();
    frame[4..6].copy_from_slice(&1u16.to_le_bytes());
    frame[HEADER_LEN] = 0xff;
    assert_eq!(
        decode_request(&frame).unwrap_err(),
        ProtocolError::UnsupportedVersion {
            received: 1,
            supported: PROTOCOL_VERSION
        }
    );
}

#[test]
fn rejects_unknown_kinds_and_flags() {
    let mut frame = encode_request(&request(1)).unwrap();
    frame[6] = 9;
    assert_eq!(
        decode_request(&frame).unwrap_err(),
        ProtocolError::UnknownFrameKind { kind: 9 }
    );

    let mut frame = encode_request(&request(1)).unwrap();
    frame[7] = 1;
    assert_eq!(
        decode_request(&frame).unwrap_err(),
        ProtocolError::UnsupportedFlags { flags: 1 }
    );
}

#[test]
fn rejects_a_frame_sent_in_the_wrong_direction() {
    let frame = encode_request(&request(1)).unwrap();
    assert_eq!(
        decode_response(&frame).unwrap_err(),
        ProtocolError::UnexpectedFrameKind {
            expected: FrameKind::Response,
            actual: FrameKind::Request
        }
    );
}

#[test]
fn rejects_truncated_payloads_and_trailing_bytes() {
    let frame = encode_request(&request(1)).unwrap();
    let truncated = &frame[..frame.len() - 1];
    assert_eq!(
        decode_request(truncated).unwrap_err(),
        ProtocolError::TruncatedPayload {
            declared: (frame.len() - HEADER_LEN) as u32,
            available: frame.len() - HEADER_LEN - 1
        }
    );

    let mut trailing = frame;
    trailing.push(0);
    assert_eq!(
        decode_request(&trailing).unwrap_err(),
        ProtocolError::TrailingBytes {
            declared: (trailing.len() - HEADER_LEN - 1) as u32,
            actual: trailing.len() - HEADER_LEN
        }
    );
}

#[test]
fn rejects_a_payload_that_is_not_a_valid_message() {
    let mut frame = encode_request(&request(1)).unwrap();
    frame[HEADER_LEN] = 0xff;
    assert!(matches!(
        decode_request(&frame).unwrap_err(),
        ProtocolError::Decode { .. }
    ));
}

#[test]
fn selects_shared_array_buffer_only_when_fully_available() {
    assert_eq!(
        select_transport(true, true),
        TransportKind::SharedArrayBuffer
    );
    assert_eq!(select_transport(true, false), TransportKind::PostMessage);
    assert_eq!(select_transport(false, true), TransportKind::PostMessage);
    assert_eq!(select_transport(false, false), TransportKind::PostMessage);
}
