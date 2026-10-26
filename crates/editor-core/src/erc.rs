//! Electrical rule checks and deterministic issue reporting.

use serde::{Deserialize, Serialize};

use crate::grid::GridPos;
use crate::schematic::ObjectRef;

/// Severity of an ERC finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ErcSeverity {
    Error,
    Warning,
    Info,
}

/// Stable ERC finding code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ErcCode {
    NoGround,
    MultipleGround,
    FloatingPin,
    StackedPins,
    DanglingWire,
    UnconnectedWireEnd,
    WireOverlap,
    JunctionOffWire,
    RedundantJunction,
    LabelOffWire,
    AmbiguousLabel,
    MultipleLabelsOnNet,
    DuplicateNetName,
    ReservedNetName,
    GroundLabelIgnored,
}

impl ErcCode {
    /// Default severity for this finding.
    pub const fn default_severity(self) -> ErcSeverity {
        match self {
            Self::NoGround
            | Self::MultipleGround
            | Self::DuplicateNetName
            | Self::ReservedNetName => ErcSeverity::Error,
            Self::StackedPins | Self::RedundantJunction | Self::GroundLabelIgnored => {
                ErcSeverity::Info
            }
            Self::FloatingPin
            | Self::DanglingWire
            | Self::UnconnectedWireEnd
            | Self::WireOverlap
            | Self::JunctionOffWire
            | Self::LabelOffWire
            | Self::AmbiguousLabel
            | Self::MultipleLabelsOnNet => ErcSeverity::Warning,
        }
    }
}

/// One ERC finding with a stable code and a human-readable message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErcIssue {
    pub severity: ErcSeverity,
    pub code: ErcCode,
    pub message: String,
    pub at: Option<GridPos>,
    pub subject: Option<ObjectRef>,
}

impl ErcIssue {
    /// Creates an issue using the code's default severity.
    pub fn new(code: ErcCode, message: impl Into<String>) -> Self {
        Self {
            severity: code.default_severity(),
            code,
            message: message.into(),
            at: None,
            subject: None,
        }
    }

    /// Attaches a grid position.
    pub fn at(mut self, position: GridPos) -> Self {
        self.at = Some(position);
        self
    }

    /// Attaches a schematic object reference.
    pub fn subject(mut self, subject: ObjectRef) -> Self {
        self.subject = Some(subject);
        self
    }
}

/// Deterministically sorted collection of ERC findings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErcReport {
    issues: Vec<ErcIssue>,
}

impl ErcReport {
    /// Creates an empty report.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a finding.
    pub fn push(&mut self, issue: ErcIssue) {
        self.issues.push(issue);
    }

    /// Sorts findings by severity, code, position, subject, and message.
    pub fn sort(&mut self) {
        self.issues.sort_by(|left, right| {
            (
                left.severity,
                left.code,
                left.at,
                left.subject,
                left.message.as_str(),
            )
                .cmp(&(
                    right.severity,
                    right.code,
                    right.at,
                    right.subject,
                    right.message.as_str(),
                ))
        });
    }

    /// Returns all findings in deterministic order.
    pub fn issues(&self) -> &[ErcIssue] {
        &self.issues
    }

    /// Number of findings.
    pub fn len(&self) -> usize {
        self.issues.len()
    }

    /// Returns true when no finding was recorded.
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns true when at least one finding is an error.
    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == ErcSeverity::Error)
    }

    /// Number of error findings.
    pub fn error_count(&self) -> usize {
        self.count_severity(ErcSeverity::Error)
    }

    /// Number of warning findings.
    pub fn warning_count(&self) -> usize {
        self.count_severity(ErcSeverity::Warning)
    }

    /// Number of informational findings.
    pub fn info_count(&self) -> usize {
        self.count_severity(ErcSeverity::Info)
    }

    fn count_severity(&self, severity: ErcSeverity) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == severity)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_errors_before_warnings_and_info() {
        let mut report = ErcReport::new();
        report.push(ErcIssue::new(ErcCode::FloatingPin, "floating"));
        report.push(ErcIssue::new(ErcCode::NoGround, "no ground"));
        report.push(ErcIssue::new(ErcCode::StackedPins, "stacked"));
        report.sort();
        assert_eq!(
            report
                .issues()
                .iter()
                .map(|issue| issue.severity)
                .collect::<Vec<_>>(),
            vec![ErcSeverity::Error, ErcSeverity::Warning, ErcSeverity::Info]
        );
        assert!(report.has_errors());
        assert_eq!(report.error_count(), 1);
        assert_eq!(report.warning_count(), 1);
        assert_eq!(report.info_count(), 1);
    }
}
