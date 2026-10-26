use sim_core::netlist::{
    ComponentId, ComponentKind, Netlist, NetlistBuilder, NodeId, NodeKind, ParamMap, ParamValue,
};
use sim_core::{AcExcitation, AcOptions, AcSweep, DcOptions, TransientOptions};
use worker::protocol::{
    decode_response, encode_request, Analysis, AnalysisFailureKind, AnalysisKind, AnalysisResult,
    ClientMessage, Job, RejectionReason, ServerMessage,
};
use worker::{
    ServiceError, SubmitOutcome, WorkerConfig, WorkerError, WorkerService, WorkerSession,
};

fn number(key: &str, value: f64) -> ParamMap {
    let mut params = ParamMap::new();
    params.insert(key.to_owned(), ParamValue::Number(value));
    params
}

fn divider() -> (Netlist, NodeId, NodeId, ComponentId) {
    let mut builder = NetlistBuilder::new();
    let ground = builder.add_node("0", NodeKind::Ground).unwrap();
    let output = builder.add_node("out", NodeKind::Electrical).unwrap();
    let source = builder
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
    (builder.build().unwrap(), output, ground, source)
}

fn job(revision: u64, netlist: Netlist, analysis: Analysis) -> Job {
    Job {
        revision,
        netlist,
        analysis,
    }
}

fn session() -> WorkerSession {
    WorkerSession::new(WorkerConfig::default()).unwrap()
}

#[test]
fn service_roundtrips_a_dc_analysis() {
    let (netlist, output, ground, _) = divider();
    let mut service = WorkerService::new(WorkerConfig::default()).unwrap();
    let request = ClientMessage::Submit {
        job: job(1, netlist, Analysis::Dc(DcOptions::default())),
    };

    let ack_frame = service
        .submit_frame(&encode_request(&request).unwrap())
        .unwrap();
    assert_eq!(
        decode_response(&ack_frame).unwrap(),
        ServerMessage::Accepted { revision: 1 }
    );

    let result_frame = service.poll_frame(10.0).unwrap().unwrap();
    let response = decode_response(&result_frame).unwrap();
    let ServerMessage::Result { revision, result } = response else {
        panic!("expected a successful result, got {response:?}");
    };
    assert_eq!(revision, 1);
    let AnalysisResult::Dc(result) = result else {
        panic!("expected a DC result");
    };
    assert!((result.node_voltage(output).unwrap() - 5.0).abs() < 1e-9);
    assert_eq!(result.node_voltage(ground), Some(0.0));
    assert!(!service.has_pending());
}

#[test]
fn ping_is_accepted_without_starting_an_analysis() {
    let mut service = WorkerService::new(WorkerConfig::default()).unwrap();
    let frame = encode_request(&ClientMessage::Ping { nonce: 17 }).unwrap();

    let reply = decode_response(&service.submit_frame(&frame).unwrap()).unwrap();
    assert_eq!(reply, ServerMessage::Pong { nonce: 17 });
    assert!(service.poll_frame(1.0).unwrap().is_none());
}

#[test]
fn coalesces_pending_jobs_and_limits_analysis_to_60_hz() {
    let (netlist, _, _, _) = divider();
    let mut session = session();
    assert!(matches!(
        session.submit(job(1, netlist.clone(), Analysis::Dc(DcOptions::default()))),
        SubmitOutcome::Accepted { revision: 1 }
    ));
    assert!(matches!(
        session.submit(job(2, netlist.clone(), Analysis::Dc(DcOptions::default()))),
        SubmitOutcome::Coalesced {
            revision: 2,
            replaced: 1
        }
    ));
    assert_eq!(session.pending_revision(), Some(2));

    let first = session.poll(100.0).unwrap().unwrap();
    assert!(matches!(first, ServerMessage::Result { revision: 2, .. }));

    assert!(matches!(
        session.submit(job(3, netlist, Analysis::Dc(DcOptions::default()))),
        SubmitOutcome::Accepted { revision: 3 }
    ));
    assert!(session.poll(100.001).unwrap().is_none());
    let second = session.poll(100.02).unwrap().unwrap();
    assert!(matches!(second, ServerMessage::Result { revision: 3, .. }));
}

#[test]
fn keeps_the_pending_slot_bounded_under_many_submissions() {
    let (netlist, _, _, _) = divider();
    let mut session = session();
    for revision in 1..=100 {
        let outcome = session.submit(job(
            revision,
            netlist.clone(),
            Analysis::Dc(DcOptions::default()),
        ));
        if revision == 1 {
            assert_eq!(outcome, SubmitOutcome::Accepted { revision: 1 });
        } else {
            assert_eq!(
                outcome,
                SubmitOutcome::Coalesced {
                    revision,
                    replaced: revision - 1
                }
            );
        }
        assert_eq!(session.pending_revision(), Some(revision));
    }

    let response = session.poll(0.0).unwrap().unwrap();
    assert!(matches!(
        response,
        ServerMessage::Result { revision: 100, .. }
    ));
}

#[test]
fn rejects_zero_duplicate_and_stale_revisions() {
    let (netlist, _, _, _) = divider();
    let mut session = session();

    assert_eq!(
        session.submit(job(0, netlist.clone(), Analysis::Dc(DcOptions::default()))),
        SubmitOutcome::Rejected {
            revision: 0,
            reason: RejectionReason::InvalidRevision
        }
    );

    assert!(matches!(
        session.submit(job(2, netlist.clone(), Analysis::Dc(DcOptions::default()))),
        SubmitOutcome::Accepted { revision: 2 }
    ));
    for revision in [1, 2] {
        assert_eq!(
            session.submit(job(
                revision,
                netlist.clone(),
                Analysis::Dc(DcOptions::default())
            )),
            SubmitOutcome::Rejected {
                revision,
                reason: RejectionReason::StaleRevision { latest: 2 }
            }
        );
    }
    assert_eq!(session.pending_revision(), Some(2));
}

#[test]
fn a_stale_revision_does_not_replace_the_pending_job() {
    let (netlist, _, _, _) = divider();
    let mut session = session();
    session.submit(job(5, netlist.clone(), Analysis::Dc(DcOptions::default())));
    assert_eq!(
        session.submit(job(3, netlist, Analysis::Dc(DcOptions::default()))),
        SubmitOutcome::Rejected {
            revision: 3,
            reason: RejectionReason::StaleRevision { latest: 5 }
        }
    );
    assert_eq!(session.pending_revision(), Some(5));
    assert!(matches!(
        session.poll(1.0).unwrap().unwrap(),
        ServerMessage::Result { revision: 5, .. }
    ));
}

#[test]
fn validates_configuration_and_the_host_clock() {
    for target_hz in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::from_bits(1)] {
        assert_eq!(
            WorkerSession::new(WorkerConfig { target_hz })
                .err()
                .unwrap(),
            WorkerError::InvalidConfig {
                reason: "target_hz must be finite and produce a finite positive period"
            }
        );
    }

    let mut session = session();
    assert!(session.poll(10.0).unwrap().is_none());
    assert_eq!(
        session.poll(9.0).unwrap_err(),
        WorkerError::TimeWentBackwards {
            previous: 10.0,
            current: 9.0
        }
    );
    assert!(matches!(
        session.poll(f64::NAN).unwrap_err(),
        WorkerError::InvalidTime { .. }
    ));
}

#[test]
fn reports_netlist_validation_failures_before_the_analysis() {
    let mut session = session();
    session.submit(job(1, Netlist::empty(), Analysis::Dc(DcOptions::default())));
    let response = session.poll(0.0).unwrap().unwrap();
    let ServerMessage::Failed { revision, failure } = response else {
        panic!("expected a netlist failure");
    };
    assert_eq!(revision, 1);
    assert_eq!(failure.kind, AnalysisFailureKind::Netlist);
    assert!(failure.message.contains("ground"));
}

#[test]
fn reports_transient_failures_with_the_correct_stage() {
    let (netlist, _, _, _) = divider();
    let mut session = session();
    session.submit(job(
        1,
        netlist,
        Analysis::Transient(TransientOptions {
            step: 0.0,
            ..TransientOptions::default()
        }),
    ));
    let response = session.poll(0.0).unwrap().unwrap();
    let ServerMessage::Failed { failure, .. } = response else {
        panic!("expected a transient failure");
    };
    assert_eq!(failure.kind, AnalysisFailureKind::Transient);
    assert!(!failure.message.is_empty());
}

#[test]
fn runs_transient_analysis_through_the_worker() {
    let (netlist, output, _, _) = divider();
    let mut session = session();
    session.submit(job(
        1,
        netlist,
        Analysis::Transient(TransientOptions {
            start_time: 0.0,
            stop_time: 2e-5,
            step: 1e-5,
            ..TransientOptions::default()
        }),
    ));

    let response = session.poll(0.0).unwrap().unwrap();
    let ServerMessage::Result {
        result: AnalysisResult::Transient(result),
        ..
    } = response
    else {
        panic!("expected a transient result");
    };
    assert_eq!(result.steps, 2);
    assert_eq!(result.samples.len(), 3);
    assert!((result.samples[2].node_voltage(output).unwrap() - 5.0).abs() < 1e-9);
}

#[test]
fn runs_ac_analysis_through_the_worker() {
    let (netlist, output, _, source) = divider();
    let mut session = session();
    session.submit(job(
        1,
        netlist,
        Analysis::Ac(AcOptions {
            sweep: AcSweep::Explicit(vec![10.0, 100.0]),
            excitations: vec![AcExcitation {
                component: source,
                magnitude: 1.0,
                phase_degrees: 0.0,
            }],
            solver: DcOptions::default(),
        }),
    ));

    let response = session.poll(0.0).unwrap().unwrap();
    let ServerMessage::Result {
        result: AnalysisResult::Ac(result),
        ..
    } = response
    else {
        panic!("expected an AC result");
    };
    assert_eq!(result.samples.len(), 2);
    for sample in &result.samples {
        let phasor = sample.node_voltage(output).unwrap();
        assert!((phasor.magnitude() - 1.0).abs() < 1e-9);
        assert!(phasor.im.abs() < 1e-9);
    }
}

#[test]
fn malformed_frames_return_typed_protocol_errors() {
    let mut service = WorkerService::new(WorkerConfig::default()).unwrap();
    assert!(matches!(
        service.submit_frame(b"nope").unwrap_err(),
        ServiceError::Protocol(_)
    ));
}

#[test]
fn protocol_messages_preserve_analysis_kind() {
    assert_eq!(Analysis::Dc(DcOptions::default()).kind(), AnalysisKind::Dc);
}
