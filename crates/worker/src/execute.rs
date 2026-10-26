//! Converts one protocol job into a completion or typed failure response.

use sim_core::{simulate_transient, solve_ac, solve_dc};
use worker_protocol::{
    Analysis, AnalysisFailure, AnalysisFailureKind, AnalysisResult, Job, ServerMessage,
};

pub(crate) fn execute_job(job: Job) -> ServerMessage {
    let revision = job.revision;
    if let Err(error) = job.netlist.validate_for_simulation() {
        return failed(revision, AnalysisFailureKind::Netlist, error.to_string());
    }

    match job.analysis {
        Analysis::Dc(options) => match solve_dc(&job.netlist, options) {
            Ok(result) => completed(revision, AnalysisResult::Dc(result)),
            Err(error) => failed(revision, AnalysisFailureKind::Dc, error.to_string()),
        },
        Analysis::Transient(options) => match simulate_transient(&job.netlist, options) {
            Ok(result) => completed(revision, AnalysisResult::Transient(result)),
            Err(error) => failed(revision, AnalysisFailureKind::Transient, error.to_string()),
        },
        Analysis::Ac(options) => match solve_ac(&job.netlist, options) {
            Ok(result) => completed(revision, AnalysisResult::Ac(result)),
            Err(error) => failed(revision, AnalysisFailureKind::Ac, error.to_string()),
        },
    }
}

fn completed(revision: u64, result: AnalysisResult) -> ServerMessage {
    ServerMessage::Result { revision, result }
}

fn failed(revision: u64, kind: AnalysisFailureKind, message: String) -> ServerMessage {
    ServerMessage::Failed {
        revision,
        failure: AnalysisFailure { kind, message },
    }
}
