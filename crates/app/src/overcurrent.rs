//! Current limits, severity levels, and overcurrent diagnostics.
//!
//! Severity is a pure function of the observed current and an editable limit so
//! that the canvas, scopes, and diagnostics always agree.

use std::cmp::Ordering;

use editor_core::{ObjectRef, Symbol};
use sim_core::netlist::ParamValue;
use sim_core::units::{format_si_precision, Unit};

use crate::analysis::{LiveValues, SignalId};
use crate::currents::{net_flows, wire_currents};
use crate::document::Document;

/// Fraction of the limit where the amber warning starts.
pub const WARNING_RATIO: f64 = 0.8;
/// Fraction of the limit where the red critical state starts.
pub const CRITICAL_RATIO: f64 = 1.0;
/// Default current allowed in a wire or net.
pub const DEFAULT_WIRE_CURRENT_LIMIT: f64 = 1.0;

/// Visual and diagnostic severity of an observed current.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Below the warning threshold, or no usable limit.
    #[default]
    None,
    /// Between 80 % and 100 % of the limit.
    Warning,
    /// Above 100 % of the limit.
    Critical,
}

impl Severity {
    /// Returns true when the value needs a visual warning.
    pub const fn is_alert(self) -> bool {
        !matches!(self, Self::None)
    }

    /// French label shown in tooltips and diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "normal",
            Self::Warning => "avertissement",
            Self::Critical => "critique",
        }
    }
}

/// Returns a usable positive limit, or `None` when it is disabled.
pub fn valid_limit(limit: f64) -> Option<f64> {
    (limit.is_finite() && limit > 0.0).then_some(limit)
}

/// Returns the editable current limit of one component.
pub fn component_limit(symbol: &Symbol) -> Option<f64> {
    symbol
        .params
        .get("imax")
        .and_then(ParamValue::as_number)
        .and_then(valid_limit)
}

/// Returns the current limit associated with a scope signal.
pub fn signal_limit(document: &Document, signal: &SignalId, wire_limit: f64) -> Option<f64> {
    match signal {
        SignalId::Current(label) => document
            .schematic()
            .symbols()
            .find(|symbol| symbol.label == *label)
            .and_then(component_limit),
        SignalId::NetCurrent(_) => valid_limit(wire_limit),
        SignalId::Voltage(_) | SignalId::ComponentVoltage(_) => None,
    }
}

/// Returns the severity of one current against an optional limit.
pub fn severity(current: f64, limit: Option<f64>) -> Severity {
    if !current.is_finite() {
        return Severity::None;
    }
    let Some(limit) = limit.and_then(valid_limit) else {
        return Severity::None;
    };
    let ratio = current.abs() / limit;
    if ratio > CRITICAL_RATIO {
        Severity::Critical
    } else if ratio >= WARNING_RATIO {
        Severity::Warning
    } else {
        Severity::None
    }
}

/// Returns the ratio of a current to a positive limit.
pub fn ratio(current: f64, limit: f64) -> f64 {
    if !current.is_finite() || !limit.is_finite() || limit <= 0.0 {
        0.0
    } else {
        current.abs() / limit
    }
}

/// One overcurrent entry shown in the diagnostics list.
#[derive(Clone, Debug, PartialEq)]
pub struct OvercurrentItem {
    /// Object selected when the item is clicked.
    pub object: ObjectRef,
    /// Component label or net name.
    pub title: String,
    /// Observed absolute current in amperes.
    pub current: f64,
    /// Limit in amperes.
    pub limit: f64,
    /// Warning or critical state.
    pub severity: Severity,
}

impl OvercurrentItem {
    /// Returns the observed current relative to the limit.
    pub fn ratio(&self) -> f64 {
        ratio(self.current, self.limit)
    }

    /// Returns the localized detail line.
    pub fn detail(&self) -> String {
        format_limit_line(self.current, self.limit, self.severity)
    }
}

/// Formats `I / Imax (percentage) · severity` for tooltips and diagnostics.
pub fn format_limit_line(current: f64, limit: f64, severity: Severity) -> String {
    format!(
        "{} / {} ({:.0} %) · {}",
        format_si_precision(current.abs(), Unit::Ampere, 3),
        format_si_precision(limit, Unit::Ampere, 3),
        ratio(current, limit) * 100.0,
        severity.label()
    )
}

/// Builds the deterministic overcurrent list for the current sample.
///
/// A stale or missing simulation intentionally produces no entry: the
/// interface must never display a safety alert from an obsolete result.
pub fn items(document: &Document, live: &LiveValues, wire_limit: f64) -> Vec<OvercurrentItem> {
    let mut items = Vec::new();
    if live.is_empty() || live.document_revision != document.revision() {
        return items;
    }

    let mut symbols = document
        .schematic()
        .symbols()
        .filter(|symbol| !symbol.kind.is_measurement() && !symbol.kind.is_ground())
        .collect::<Vec<_>>();
    symbols.sort_by(|left, right| {
        left.label
            .cmp(&right.label)
            .then_with(|| left.id.0.cmp(&right.id.0))
    });
    for symbol in symbols {
        let Some(limit) = component_limit(symbol) else {
            continue;
        };
        let Some(current) = live.symbol(document, symbol).current else {
            continue;
        };
        let severity = severity(current, Some(limit));
        if severity.is_alert() {
            items.push(OvercurrentItem {
                object: ObjectRef::Symbol(symbol.id),
                title: symbol.label.clone(),
                current: current.abs(),
                limit,
                severity,
            });
        }
    }

    let currents = wire_currents(document, live);
    let flows = net_flows(document, live);
    for net in document.nets() {
        let mut current = net
            .wires
            .iter()
            .filter_map(|id| currents.get(id).copied())
            .map(f64::abs)
            .fold(0.0_f64, f64::max);
        current = current.max(
            flows
                .get(net.name.as_str())
                .map_or(0.0, |flow| flow.strength),
        );
        let Some(limit) = valid_limit(wire_limit) else {
            continue;
        };
        let severity = severity(current, Some(limit));
        if !severity.is_alert() {
            continue;
        }
        let representative = net.wires.iter().copied().max_by(|left_id, right_id| {
            let left = currents.get(left_id).copied().unwrap_or(0.0).abs();
            let right = currents.get(right_id).copied().unwrap_or(0.0).abs();
            left.partial_cmp(&right)
                .unwrap_or(Ordering::Equal)
                .then_with(|| right_id.0.cmp(&left_id.0))
        });
        let Some(wire) = representative else {
            continue;
        };
        items.push(OvercurrentItem {
            object: ObjectRef::Wire(wire),
            title: format!("Net {}", net.name),
            current,
            limit,
            severity,
        });
    }

    items.sort_by(|left, right| {
        right
            .severity
            .cmp(&left.severity)
            .then_with(|| {
                right
                    .ratio()
                    .partial_cmp(&left.ratio())
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| left.title.cmp(&right.title))
    });
    items
}

/// Returns contiguous polyline segments whose absolute value exceeds a limit.
pub fn over_limit_segments(points: &[[f64; 2]], limit: f64) -> Vec<Vec<[f64; 2]>> {
    let Some(limit) = valid_limit(limit) else {
        return Vec::new();
    };
    let mut segments = Vec::new();
    let mut current = Vec::new();
    for (index, point) in points.iter().enumerate() {
        let over = point[1].is_finite() && point[1].abs() > limit;
        if over {
            if current.is_empty() && index > 0 {
                current.push(points[index - 1]);
            }
            current.push(*point);
        } else if !current.is_empty() {
            current.push(*point);
            segments.push(std::mem::take(&mut current));
        }
    }
    if current.len() >= 2 {
        segments.push(current);
    }
    segments
}

/// Accumulated over-limit statistics for one trace.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OverLimitStats {
    /// Largest absolute value above the limit.
    pub peak: f64,
    /// Total time spent at or above the limit.
    pub duration: f64,
    /// Number of samples above the limit.
    pub samples: usize,
}

impl OverLimitStats {
    /// Returns the peak relative to the limit.
    pub fn ratio(&self, limit: f64) -> f64 {
        ratio(self.peak, limit)
    }
}

/// Computes peak, duration, and sample count above a limit.
pub fn over_limit_stats(points: &[[f64; 2]], limit: f64) -> Option<OverLimitStats> {
    let limit = valid_limit(limit)?;
    let mut stats = OverLimitStats::default();
    let mut previous_time = points.first()?[0];
    let mut previous_over = false;
    for point in points {
        let over = point[1].is_finite() && point[1].abs() > limit;
        if over {
            stats.samples += 1;
            stats.peak = stats.peak.max(point[1].abs());
        }
        let dt = point[0] - previous_time;
        if dt.is_finite() && dt > 0.0 && (over || previous_over) {
            stats.duration += dt;
        }
        previous_over = over;
        previous_time = point[0];
    }
    (stats.samples > 0).then_some(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::{GridPos, Orientation, SymbolId, SymbolKind, SymbolRegistry};

    fn resistor(imax: f64) -> Symbol {
        let registry = SymbolRegistry::builtin();
        let definition = registry.get(&SymbolKind::Resistor).unwrap();
        let mut symbol = Symbol::from_def(
            SymbolId(1),
            definition,
            GridPos::new(0, 0),
            "R1",
            Orientation::R0,
            false,
        );
        symbol
            .params
            .insert("imax".to_owned(), ParamValue::Number(imax));
        symbol
    }

    #[test]
    fn severity_thresholds_are_warning_then_critical() {
        assert_eq!(severity(0.79, Some(1.0)), Severity::None);
        assert_eq!(severity(0.8, Some(1.0)), Severity::Warning);
        assert_eq!(severity(1.0, Some(1.0)), Severity::Warning);
        assert_eq!(severity(1.001, Some(1.0)), Severity::Critical);
        assert_eq!(severity(-2.0, Some(1.0)), Severity::Critical);
        assert_eq!(severity(2.0, None), Severity::None);
        assert_eq!(severity(2.0, Some(0.0)), Severity::None);
    }

    #[test]
    fn component_limit_comes_from_imax() {
        assert_eq!(component_limit(&resistor(0.25)), Some(0.25));
        let mut symbol = resistor(0.25);
        symbol.params.remove("imax");
        assert_eq!(component_limit(&symbol), None);
    }

    #[test]
    fn over_limit_stats_report_peak_and_duration() {
        let points = [
            [0.0, 0.5],
            [1e-3, 1.2],
            [2e-3, 1.6],
            [3e-3, 0.4],
            [4e-3, 1.1],
        ];
        let stats = over_limit_stats(&points, 1.0).unwrap();
        assert!((stats.peak - 1.6).abs() < 1e-12);
        assert_eq!(stats.samples, 3);
        assert!(over_limit_stats(&[[0.0, 0.5]], 1.0).is_none());
        let segments = over_limit_segments(&points, 1.0);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].len(), 4);
        assert_eq!(segments[1].len(), 2);
        assert!((stats.duration - 4e-3).abs() < 1e-12);
    }

    #[test]
    fn stale_or_missing_samples_never_raise_an_alert() {
        let document = Document::demo().unwrap();
        let resistor = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == SymbolKind::Resistor)
            .unwrap()
            .clone();
        let definition = document.registry().get(&resistor.kind).unwrap();
        let a = resistor
            .pin_position_of(&definition.pins[0])
            .and_then(|at| document.net_name_at(at))
            .unwrap();
        let b = resistor
            .pin_position_of(&definition.pins[1])
            .and_then(|at| document.net_name_at(at))
            .unwrap();
        let live = crate::analysis::LiveValues {
            document_revision: document.revision(),
            time: Some(1e-3),
            node_voltages: [(a, 300.0), (b, 0.0)].into_iter().collect(),
            branch_currents: Default::default(),
        };
        assert!(items(&document, &live, 1.0)
            .iter()
            .any(|item| item.object == ObjectRef::Symbol(resistor.id)));
        assert!(items(&document, &crate::analysis::LiveValues::default(), 1.0).is_empty());
        let mut stale = live;
        stale.document_revision = stale.document_revision.wrapping_add(1);
        assert!(items(&document, &stale, 1.0).is_empty());
    }

    #[test]
    fn net_signals_use_the_configured_wire_limit() {
        let document = Document::demo().unwrap();
        assert_eq!(
            signal_limit(&document, &SignalId::NetCurrent("OUT".to_owned()), 0.5),
            Some(0.5)
        );
        assert_eq!(
            signal_limit(&document, &SignalId::NetCurrent("OUT".to_owned()), 0.0),
            None
        );
        assert_eq!(
            signal_limit(&document, &SignalId::Voltage("OUT".to_owned()), 0.5),
            None
        );
    }
}
