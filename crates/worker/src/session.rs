//! Single-slot worker scheduling with revision checks and 60 Hz coalescing.

use thiserror::Error;

use worker_protocol::{Job, RejectionReason, ServerMessage};

use crate::execute::execute_job;

/// Default maximum analysis start rate.
pub const DEFAULT_TARGET_HZ: f64 = 60.0;

/// Scheduling parameters for one worker session.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkerConfig {
    /// Maximum number of analysis starts per second.
    pub target_hz: f64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            target_hz: DEFAULT_TARGET_HZ,
        }
    }
}

impl WorkerConfig {
    /// Rejects rates that cannot produce a finite, positive period.
    pub fn validate(&self) -> Result<(), WorkerError> {
        let period = 1.0 / self.target_hz;
        if !self.target_hz.is_finite()
            || self.target_hz <= 0.0
            || !period.is_finite()
            || period <= 0.0
        {
            return Err(WorkerError::InvalidConfig {
                reason: "target_hz must be finite and produce a finite positive period",
            });
        }
        Ok(())
    }

    fn period_seconds(self) -> f64 {
        1.0 / self.target_hz
    }
}

/// Worker configuration, clock, or scheduling error.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum WorkerError {
    /// The scheduling configuration cannot be used.
    #[error("invalid worker configuration: {reason}")]
    InvalidConfig {
        /// Description of the invalid constraint.
        reason: &'static str,
    },
    /// The host supplied a non-finite timestamp.
    #[error("worker time must be finite, got {seconds}")]
    InvalidTime {
        /// Invalid timestamp in seconds.
        seconds: f64,
    },
    /// The host clock moved backwards between two polls.
    #[error("worker time moved backwards from {previous} to {current} seconds")]
    TimeWentBackwards {
        /// Previous accepted timestamp.
        previous: f64,
        /// New timestamp smaller than `previous`.
        current: f64,
    },
    /// The next scheduled deadline is not finite.
    #[error("worker schedule overflowed the representable time range")]
    ScheduleOverflow,
}

/// Result of placing a job in the bounded pending slot.
#[derive(Clone, Debug, PartialEq)]
pub enum SubmitOutcome {
    /// The slot was empty and now contains the job.
    Accepted {
        /// Revision stored by the worker.
        revision: u64,
    },
    /// The new job replaced an older pending job.
    Coalesced {
        /// Revision now stored by the worker.
        revision: u64,
        /// Older revision discarded without execution.
        replaced: u64,
    },
    /// The job was refused and the pending slot is unchanged.
    Rejected {
        /// Refused revision.
        revision: u64,
        /// Reason for the refusal.
        reason: RejectionReason,
    },
}

/// Deterministic single-threaded worker state.
///
/// The host supplies every timestamp; the session never reads a clock. At most
/// one job is pending, so submissions cannot build an unbounded queue.
pub struct WorkerSession {
    config: WorkerConfig,
    pending: Option<Job>,
    next_run: Option<f64>,
    last_time: Option<f64>,
    latest_revision: u64,
}

impl WorkerSession {
    /// Creates an empty session after validating its scheduling configuration.
    pub fn new(config: WorkerConfig) -> Result<Self, WorkerError> {
        config.validate()?;
        Ok(Self {
            config,
            pending: None,
            next_run: None,
            last_time: None,
            latest_revision: 0,
        })
    }

    /// Returns the validated scheduling configuration.
    pub const fn config(&self) -> WorkerConfig {
        self.config
    }

    /// Returns the latest revision accepted by this session.
    pub const fn latest_revision(&self) -> u64 {
        self.latest_revision
    }

    /// Returns the pending revision, if any.
    pub fn pending_revision(&self) -> Option<u64> {
        self.pending.as_ref().map(|job| job.revision)
    }

    /// Returns true when a job is waiting for its next allowed start time.
    pub const fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Stores a job, coalescing it with the previous pending job when present.
    pub fn submit(&mut self, job: Job) -> SubmitOutcome {
        let revision = job.revision;
        if revision == 0 {
            return SubmitOutcome::Rejected {
                revision,
                reason: RejectionReason::InvalidRevision,
            };
        }
        if revision <= self.latest_revision {
            return SubmitOutcome::Rejected {
                revision,
                reason: RejectionReason::StaleRevision {
                    latest: self.latest_revision,
                },
            };
        }
        self.latest_revision = revision;
        if let Some(previous) = self.pending.replace(job) {
            SubmitOutcome::Coalesced {
                revision,
                replaced: previous.revision,
            }
        } else {
            SubmitOutcome::Accepted { revision }
        }
    }

    /// Runs the pending job when the 60 Hz gate and host clock allow it.
    ///
    /// Returns `None` when no work is pending or the next deadline is later than
    /// `now_seconds`. The pending job is retained until a run starts.
    pub fn poll(&mut self, now_seconds: f64) -> Result<Option<ServerMessage>, WorkerError> {
        if !now_seconds.is_finite() {
            return Err(WorkerError::InvalidTime {
                seconds: now_seconds,
            });
        }
        if let Some(previous) = self.last_time {
            if now_seconds < previous {
                return Err(WorkerError::TimeWentBackwards {
                    previous,
                    current: now_seconds,
                });
            }
        }
        self.last_time = Some(now_seconds);

        if self.pending.is_none() {
            return Ok(None);
        }
        if self.next_run.is_some_and(|deadline| now_seconds < deadline) {
            return Ok(None);
        }
        let next_run = now_seconds + self.config.period_seconds();
        if !next_run.is_finite() {
            return Err(WorkerError::ScheduleOverflow);
        }
        let Some(job) = self.pending.take() else {
            return Ok(None);
        };
        let response = execute_job(job);
        self.next_run = Some(next_run);
        Ok(Some(response))
    }
}
