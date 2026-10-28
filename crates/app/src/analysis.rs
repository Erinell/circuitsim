//! Simulation preparation, worker execution, traces, measurements, and FFT.

use std::collections::BTreeMap;

use editor_core::{Extraction, Symbol, SymbolId, SymbolKind};
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use sim_core::netlist::{
    ComponentId, ComponentKind, ComponentSpec, Netlist, NetlistError, NodeId, ParamValue,
};
use sim_core::units::{format_si_precision, parse_si, Unit};
use sim_core::{
    AcExcitation, AcOptions, AcResult, AcSweep, DcOptions, TransientAction, TransientEvent,
    TransientOptions, TransientResult, TransientSample,
};
use thiserror::Error;
use worker::protocol::{
    decode_response, encode_request, Analysis, AnalysisFailureKind, AnalysisKind, AnalysisResult,
    ClientMessage, Job, RejectionReason, ServerMessage,
};
use worker::{WorkerConfig, WorkerService};

/// Maximum number of transient intervals accepted by the interface.
pub const MAX_TRANSIENT_STEPS: usize = 100_000;
/// Maximum number of AC sweep points accepted by the interface.
pub const MAX_AC_POINTS: usize = 10_000;
/// Maximum power-of-two spectrum size computed by the interface.
pub const MAX_FFT_POINTS: usize = 16_384;
/// Sub-picovolt values are numerical noise rather than useful traces.
const VOLTAGE_TRACE_FLOOR: f64 = 1e-12;
/// Sub-femtovolt currents are numerical noise rather than useful traces.
const CURRENT_TRACE_FLOOR: f64 = 1e-15;
/// Returns the floor below which a trace value is displayed as zero.
fn trace_floor(signal: &SignalId) -> f64 {
    match signal {
        SignalId::Voltage(_) | SignalId::ComponentVoltage(_) => VOLTAGE_TRACE_FLOOR,
        SignalId::NetCurrent(_) | SignalId::Current(_) => CURRENT_TRACE_FLOOR,
    }
}

/// Voltage or current displayed by an instrument.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SignalId {
    /// Voltage of a named net.
    Voltage(String),
    /// Voltage across a named two-terminal component.
    ComponentVoltage(String),
    /// Current flowing in a named net.
    NetCurrent(String),
    /// Branch current of a named component.
    Current(String),
}

impl SignalId {
    /// Returns the plot label, such as `V(OUT)` or `I(OUT)`.
    pub fn label(&self) -> String {
        match self {
            Self::Voltage(name) => format!("V({name})"),
            Self::ComponentVoltage(name) => format!("U({name})"),
            Self::NetCurrent(name) => format!("I({name})"),
            Self::Current(name) => format!("I({name})"),
        }
    }

    /// Returns the physical unit of this signal.
    pub const fn unit(&self) -> SignalUnit {
        match self {
            Self::Voltage(_) | Self::ComponentVoltage(_) => SignalUnit::Volt,
            Self::NetCurrent(_) | Self::Current(_) => SignalUnit::Ampere,
        }
    }
}

/// Unit carried by a trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalUnit {
    /// Volts.
    Volt,
    /// Amperes.
    Ampere,
}

impl SignalUnit {
    /// Returns the SI display unit.
    pub const fn unit(self) -> Unit {
        match self {
            Self::Volt => Unit::Volt,
            Self::Ampere => Unit::Ampere,
        }
    }

    /// Returns the short display symbol.
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Volt => "V",
            Self::Ampere => "A",
        }
    }
}

/// Independent source offered by the simulation controls.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceChoice {
    /// Component label used to identify the source.
    pub label: String,
    /// Current DC value in SI units.
    pub dc: f64,
    /// Unit of the source value.
    pub unit: Unit,
}

impl SourceChoice {
    /// Formats the DC value with its engineering prefix.
    pub fn formatted_dc(&self) -> String {
        format_si_precision(self.dc, self.unit, 4)
    }
}

/// Signals and independent sources extracted from the current schematic.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SignalCatalog {
    /// Named electrical nets, sorted by name and excluding ground.
    pub voltages: Vec<String>,
    /// Branch-current components, sorted by label.
    pub currents: Vec<String>,
    /// Components with a defined primary voltage, sorted by label.
    pub components: Vec<String>,
    /// Independent voltage and current sources, sorted by label.
    pub sources: Vec<SourceChoice>,
}

impl SignalCatalog {
    /// Builds a deterministic catalog from a validated extraction.
    pub fn from_extraction(extraction: &Extraction) -> Self {
        let mut voltages = extraction
            .nets
            .iter()
            .filter(|net| net.name.as_str() != "0")
            .map(|net| net.name.as_str().to_owned())
            .collect::<Vec<_>>();
        voltages.sort();
        voltages.dedup();

        let mut currents = extraction
            .netlist
            .components
            .iter()
            .filter(|component| has_branch_current(component))
            .map(|component| component.label.clone())
            .collect::<Vec<_>>();
        currents.sort();
        currents.dedup();

        let mut components = extraction
            .netlist
            .components
            .iter()
            .filter(|component| {
                component.pins.len() == 2
                    || matches!(
                        component.kind,
                        ComponentKind::ShuntReference
                            | ComponentKind::Comparator
                            | ComponentKind::Mosfet
                    )
            })
            .map(|component| component.label.clone())
            .collect::<Vec<_>>();
        components.sort();
        components.dedup();

        let mut sources = extraction
            .netlist
            .components
            .iter()
            .filter_map(|component| match component.kind {
                ComponentKind::VoltageSource | ComponentKind::CurrentSource => {
                    let unit = if component.kind == ComponentKind::VoltageSource {
                        Unit::Volt
                    } else {
                        Unit::Ampere
                    };
                    let dc = component
                        .params
                        .get("dc")
                        .and_then(ParamValue::as_number)
                        .unwrap_or(0.0);
                    Some(SourceChoice {
                        label: component.label.clone(),
                        dc,
                        unit,
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        sources.sort_by(|left, right| left.label.cmp(&right.label));

        Self {
            voltages,
            currents,
            components,
            sources,
        }
    }

    /// Returns true when the catalog contains the signal.
    pub fn contains(&self, signal: &SignalId) -> bool {
        match signal {
            SignalId::Voltage(name) | SignalId::NetCurrent(name) => self.voltages.contains(name),
            SignalId::ComponentVoltage(name) => self.components.contains(name),
            SignalId::Current(name) => self.currents.contains(name),
        }
    }

    /// Returns all available signals with voltages first.
    pub fn signals(&self) -> Vec<SignalId> {
        self.voltages
            .iter()
            .cloned()
            .map(SignalId::Voltage)
            .chain(self.currents.iter().cloned().map(SignalId::Current))
            .collect()
    }

    /// Returns the preferred voltage display, favouring `OUT` then `Vc`.
    pub fn default_voltage(&self) -> Option<String> {
        for preferred in ["OUT", "Vc"] {
            if self.voltages.iter().any(|name| name == preferred) {
                return Some(preferred.to_owned());
            }
        }
        self.voltages.first().cloned()
    }
}

fn has_branch_current(component: &ComponentSpec) -> bool {
    matches!(
        component.kind,
        ComponentKind::VoltageSource | ComponentKind::Inductor | ComponentKind::OpAmp
    )
}

/// Editable transient settings stored as user-facing engineering text.
#[derive(Clone, Debug, PartialEq)]
pub struct TransientSettings {
    /// Start time in seconds.
    pub start_time: String,
    /// Stop time in seconds.
    pub stop_time: String,
    /// Fixed integration step in seconds.
    pub step: String,
    /// Enables the source-step helper.
    pub step_enabled: bool,
    /// Selected independent source.
    pub source: Option<String>,
    /// Source value before the step.
    pub initial_value: String,
    /// Instant of the step in seconds.
    pub event_time: String,
    /// Source value after the step.
    pub final_value: String,
    last_source: Option<String>,
}

impl Default for TransientSettings {
    fn default() -> Self {
        Self {
            start_time: "0".to_owned(),
            stop_time: "5m".to_owned(),
            step: "10u".to_owned(),
            step_enabled: false,
            source: None,
            initial_value: "0".to_owned(),
            event_time: "0".to_owned(),
            final_value: String::new(),
            last_source: None,
        }
    }
}

impl TransientSettings {
    /// Keeps the selected source valid and initializes the step target.
    pub fn ensure_catalog(&mut self, sources: &[SourceChoice]) {
        let next = self
            .source
            .as_ref()
            .filter(|label| sources.iter().any(|source| &source.label == *label))
            .cloned()
            .or_else(|| sources.first().map(|source| source.label.clone()));
        if self.last_source != next {
            if let Some(source) = next
                .as_ref()
                .and_then(|label| sources.iter().find(|source| &source.label == label))
            {
                self.final_value = source.formatted_dc();
            }
            self.last_source = next.clone();
            self.source = next;
        }
        if sources.is_empty() {
            self.step_enabled = false;
        }
    }
}

/// Editable AC sweep settings stored as user-facing engineering text.
#[derive(Clone, Debug, PartialEq)]
pub struct AcSettings {
    /// Selected independent source.
    pub source: Option<String>,
    /// Preferred output net for Bode display.
    pub output: Option<String>,
    /// Excitation peak amplitude.
    pub amplitude: String,
    /// First frequency in hertz.
    pub start: String,
    /// Last frequency in hertz.
    pub stop: String,
    /// Number of logarithmically spaced points.
    pub points: usize,
}

impl Default for AcSettings {
    fn default() -> Self {
        Self {
            source: None,
            output: None,
            amplitude: "1".to_owned(),
            start: "1".to_owned(),
            stop: "1M".to_owned(),
            points: 101,
        }
    }
}

impl AcSettings {
    /// Keeps the selected source and output valid for the current schematic.
    pub fn ensure_catalog(&mut self, catalog: &SignalCatalog) {
        if !self
            .source
            .as_ref()
            .is_some_and(|label| catalog.sources.iter().any(|source| &source.label == label))
        {
            self.source = catalog.sources.first().map(|source| source.label.clone());
        }
        if !self
            .output
            .as_ref()
            .is_some_and(|name| catalog.voltages.contains(name))
        {
            self.output = catalog.default_voltage();
        }
    }
}

/// Netlist and options prepared for one worker submission.
#[derive(Debug)]
pub struct PreparedAnalysis {
    netlist: Netlist,
    analysis: Analysis,
    context: RunContext,
}

/// Names and revision needed to interpret a worker result.
#[derive(Clone, Debug)]
pub struct RunContext {
    /// Document revision at submission time.
    pub document_revision: u64,
    /// Net names mapped to netlist node identifiers.
    pub nodes: BTreeMap<String, NodeId>,
    /// Component labels mapped to netlist identifiers.
    pub components: BTreeMap<String, ComponentId>,
    /// AC excitation amplitude used for gain normalization.
    pub input_amplitude: Option<f64>,
    /// Requested transient step used by the FFT resampler.
    pub nominal_step: Option<f64>,
}

/// Completed worker analysis with its interpretation context.
#[derive(Clone, Debug)]
pub struct CompletedRun {
    /// Worker request revision.
    pub revision: u64,
    /// Analysis family.
    pub kind: AnalysisKind,
    /// Solver output.
    pub result: AnalysisResult,
    /// Net and component maps captured before execution.
    pub context: RunContext,
}

/// Values sampled from the most recent transient result for the inspector.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveValues {
    /// Document revision captured when the run was submitted.
    pub document_revision: u64,
    /// Time of the displayed sample.
    pub time: Option<f64>,
    /// Net voltages indexed by net name.
    pub node_voltages: BTreeMap<String, f64>,
    /// Branch currents indexed by component label.
    pub branch_currents: BTreeMap<String, f64>,
}

/// Voltage and current observed on one symbol.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SymbolLive {
    /// Transformed pins with their net name and voltage.
    pub pins: Vec<PinLive>,
    /// Primary component voltage, using K-to-A or OUT-to-VSS for three-terminal models.
    pub voltage: Option<f64>,
    /// Branch or resistor current when it can be derived.
    pub current: Option<f64>,
    /// Power when both voltage and current are known.
    pub power: Option<f64>,
}

/// One symbol pin together with its net voltage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PinLive {
    /// Pin name from the symbol definition.
    pub name: String,
    /// Electrical net attached to the pin.
    pub net: Option<String>,
    /// Voltage of the net in volts.
    pub voltage: Option<f64>,
}

/// Measurement reported by a placed probe.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeReading {
    /// Probe symbol identifier.
    pub id: SymbolId,
    /// Probe reference such as `VM1`.
    pub label: String,
    /// Probe kind.
    pub kind: SymbolKind,
    /// Net observed by a voltmeter.
    pub net: Option<String>,
    /// Component observed by an ammeter or ohmmeter.
    pub target: Option<String>,
    /// Measured value in SI units.
    pub value: Option<f64>,
    /// Physical unit of the value.
    pub unit: Option<Unit>,
}

impl ProbeReading {
    /// Returns the signal that can be plotted for this probe.
    pub fn signal(&self) -> Option<SignalId> {
        match self.kind {
            SymbolKind::Voltmeter => self.net.clone().map(SignalId::Voltage),
            SymbolKind::Ammeter => self.target.clone().map(SignalId::Current),
            _ => None,
        }
    }
}

/// Computes all probe readings from the last transient sample.
pub fn probe_readings(
    document: &crate::document::Document,
    live: &LiveValues,
) -> Vec<ProbeReading> {
    document
        .schematic()
        .symbols()
        .filter(|symbol| symbol.kind.is_measurement())
        .map(|probe| {
            let net = document.electrical_net_name_at(probe.at);
            let target = document.component_at(probe.at);
            let (value, unit, target_label) = match probe.kind {
                SymbolKind::Voltmeter => (
                    net.as_deref().and_then(|name| live.voltage(name)),
                    Some(Unit::Volt),
                    None,
                ),
                SymbolKind::Ammeter => {
                    let current = target.and_then(|target| live.symbol(document, target).current);
                    (
                        current,
                        Some(Unit::Ampere),
                        target.map(|target| target.label.clone()),
                    )
                }
                SymbolKind::Ohmmeter => {
                    let resistance = target.and_then(|target| {
                        let fixed = match target.kind {
                            SymbolKind::Resistor => target
                                .params
                                .get("resistance")
                                .and_then(ParamValue::as_number),
                            SymbolKind::Switch => {
                                let closed = target
                                    .params
                                    .get("closed")
                                    .and_then(ParamValue::as_bool)
                                    .unwrap_or(false);
                                target
                                    .params
                                    .get(if closed { "ron" } else { "roff" })
                                    .and_then(ParamValue::as_number)
                            }
                            _ => None,
                        };
                        fixed.or_else(|| {
                            let observed = live.symbol(document, target);
                            let voltage = match (observed.pins.first(), observed.pins.get(1)) {
                                (Some(first), Some(second)) => first.voltage.zip(second.voltage),
                                _ => None,
                            }?;
                            let current = observed.current?;
                            (current.abs() > f64::MIN_POSITIVE)
                                .then_some((voltage.0 - voltage.1) / current)
                        })
                    });
                    (
                        resistance,
                        Some(Unit::Ohm),
                        target.map(|target| target.label.clone()),
                    )
                }
                _ => (None, None, None),
            };
            ProbeReading {
                id: probe.id,
                label: probe.label.clone(),
                kind: probe.kind.clone(),
                net,
                target: target_label,
                value,
                unit,
            }
        })
        .collect()
}

impl LiveValues {
    /// Returns true when no sample is available.
    pub fn is_empty(&self) -> bool {
        self.time.is_none() && self.node_voltages.is_empty() && self.branch_currents.is_empty()
    }

    /// Returns a net voltage by name.
    pub fn voltage(&self, net: &str) -> Option<f64> {
        self.node_voltages.get(net).copied()
    }

    /// Returns a branch current by component label.
    pub fn current(&self, label: &str) -> Option<f64> {
        self.branch_currents.get(label).copied()
    }

    /// Resolves observed values for one placed symbol.
    pub fn symbol(&self, document: &crate::document::Document, symbol: &Symbol) -> SymbolLive {
        let Some(extraction) = document.extraction() else {
            return SymbolLive::default();
        };
        let Some(definition) = document.registry().get(&symbol.kind) else {
            return SymbolLive::default();
        };
        let mut pins = Vec::with_capacity(definition.pins.len());
        for (index, pin) in definition.pins.iter().enumerate() {
            let key = editor_core::PinKey {
                symbol: symbol.id,
                pin: u16::try_from(index).unwrap_or(u16::MAX),
            };
            let net = extraction.net_of_pin.get(&key).and_then(|node| {
                extraction
                    .nets
                    .iter()
                    .find(|net| net.node == *node)
                    .map(|net| net.name.as_str().to_owned())
            });
            let voltage = net.as_deref().and_then(|name| self.voltage(name));
            pins.push(PinLive {
                name: pin.name.clone(),
                net,
                voltage,
            });
        }
        let (positive_pin, negative_pin) = match symbol.kind {
            SymbolKind::ShuntReference => (pins.get(1), pins.get(2)),
            SymbolKind::Comparator => (pins.get(2), pins.get(4)),
            SymbolKind::Mosfet => (pins.first(), pins.get(2)),
            _ => (pins.first(), pins.get(1)),
        };
        let across = match (positive_pin, negative_pin) {
            (Some(first), Some(second)) => match (first.voltage, second.voltage) {
                (Some(a), Some(b)) => Some(a - b),
                _ => None,
            },
            _ => None,
        };
        let control_voltage = match symbol.kind {
            SymbolKind::ShuntReference => match (pins.first(), pins.get(2)) {
                (Some(reference), Some(anode)) => reference
                    .voltage
                    .zip(anode.voltage)
                    .map(|(reference, anode)| reference - anode),
                _ => None,
            },
            SymbolKind::Comparator => match (pins.first(), pins.get(1), pins.get(3), pins.get(4)) {
                (Some(positive), Some(negative), Some(vcc), Some(vss)) => positive
                    .voltage
                    .zip(negative.voltage)
                    .zip(vcc.voltage.zip(vss.voltage))
                    .map(|((positive, negative), (vcc, vss))| {
                        if (2.0..=36.0).contains(&(vcc - vss)) {
                            positive - negative
                        } else {
                            1.0
                        }
                    }),
                _ => None,
            },
            SymbolKind::Mosfet => match (pins.get(1), pins.get(2)) {
                (Some(gate), Some(source)) => gate
                    .voltage
                    .zip(source.voltage)
                    .map(|(gate, source)| gate - source),
                _ => None,
            },
            _ => None,
        };
        let current = self.current(&symbol.label).or_else(|| {
            across.and_then(|voltage| derive_current(symbol, voltage, control_voltage))
        });
        let power = if symbol.kind == SymbolKind::ShuntReference {
            across.zip(current).map(|(voltage, current)| {
                let reference_current = symbol
                    .params
                    .get("iref")
                    .and_then(ParamValue::as_number)
                    .unwrap_or(0.0);
                let reference_voltage = match (pins.first(), pins.get(2)) {
                    (Some(reference), Some(anode)) => reference
                        .voltage
                        .zip(anode.voltage)
                        .map(|(reference, anode)| reference - anode)
                        .unwrap_or(0.0),
                    _ => 0.0,
                };
                voltage * current + reference_voltage * reference_current
            })
        } else {
            across
                .zip(current)
                .map(|(voltage, current)| voltage * current)
        };
        SymbolLive {
            pins,
            voltage: across,
            current,
            power,
        }
    }
}

fn derive_current(symbol: &Symbol, voltage: f64, reference_voltage: Option<f64>) -> Option<f64> {
    if symbol.kind == SymbolKind::Switch
        && !symbol
            .params
            .get("closed")
            .and_then(ParamValue::as_bool)
            .unwrap_or(false)
    {
        return Some(0.0);
    }
    let resistance = match symbol.kind {
        SymbolKind::Resistor => symbol
            .params
            .get("resistance")
            .and_then(ParamValue::as_number),
        SymbolKind::Switch => symbol.params.get("ron").and_then(ParamValue::as_number),
        SymbolKind::CurrentSource => {
            return symbol.params.get("dc").and_then(ParamValue::as_number);
        }
        SymbolKind::Diode | SymbolKind::Led | SymbolKind::Zener => {
            return diode_params_current(&symbol.params, symbol.kind == SymbolKind::Zener, voltage);
        }
        SymbolKind::ShuntReference => {
            return shunt_reference_current(&symbol.params, reference_voltage?, voltage);
        }
        SymbolKind::Comparator => {
            return comparator_sink_current(&symbol.params, reference_voltage?, voltage);
        }
        SymbolKind::Mosfet => {
            return mosfet_current(&symbol.params, voltage, reference_voltage?);
        }
        _ => None,
    }?;
    (resistance.abs() > f64::MIN_POSITIVE).then_some(voltage / resistance)
}

fn shunt_reference_current(
    params: &sim_core::netlist::ParamMap,
    reference_voltage: f64,
    cathode_voltage: f64,
) -> Option<f64> {
    let number = |key: &str| {
        params
            .get(key)
            .and_then(ParamValue::as_number)
            .filter(|value| value.is_finite())
    };
    let vref = number("vref")?;
    let minimum_current = number("ik_min")?;
    let maximum_current = number("imax")?;
    let dynamic_impedance = number("z_ka")?;
    if vref <= 0.0
        || minimum_current <= 0.0
        || maximum_current < minimum_current
        || dynamic_impedance <= 0.0
    {
        return None;
    }
    let commanded = (minimum_current + (reference_voltage - vref) / dynamic_impedance)
        .clamp(0.0, maximum_current);
    let headroom = (cathode_voltage / vref).clamp(0.0, 1.0);
    Some(commanded * headroom * headroom * (3.0 - 2.0 * headroom))
}

pub(crate) fn symbol_terminal_injections(symbol: &Symbol, current: f64) -> [f64; 5] {
    if symbol.kind == SymbolKind::Comparator {
        return [0.0, 0.0, -current, 0.0, current];
    }
    if symbol.kind == SymbolKind::Mosfet {
        // Drain, gate, source: the channel current enters the drain and leaves
        // through the source, and the gate draws no steady current.
        return [-current, 0.0, current, 0.0, 0.0];
    }
    if symbol.kind == SymbolKind::ShuntReference {
        let reference_current = symbol
            .params
            .get("iref")
            .and_then(ParamValue::as_number)
            .filter(|value| value.is_finite())
            .unwrap_or(0.0);
        return [
            -reference_current,
            -current,
            current + reference_current,
            0.0,
            0.0,
        ];
    }
    [-current, current, 0.0, 0.0, 0.0]
}

/// Steady-state drain current of the level-1 MOSFET for the live overlay.
///
/// Uses the same square law and body diode as the simulator, in symbol
/// coordinates: `vds` is the drain-to-source pin voltage and `vgs` the
/// gate-to-source pin voltage.
fn mosfet_current(params: &sim_core::netlist::ParamMap, vds: f64, vgs: f64) -> Option<f64> {
    let number = |key: &str| {
        params
            .get(key)
            .and_then(ParamValue::as_number)
            .filter(|value| value.is_finite())
    };
    let channel = params.get("channel").and_then(ParamValue::as_text)?;
    let polarity = if channel.eq_ignore_ascii_case("n") {
        1.0
    } else if channel.eq_ignore_ascii_case("p") {
        -1.0
    } else {
        return None;
    };
    let threshold = number("vth")?;
    let gain = number("k")?;
    let lambda = number("lambda").unwrap_or(0.0);
    let saturation_current = number("is").unwrap_or(1e-14);
    let emission = number("n").unwrap_or(1.0);
    if threshold <= 0.0
        || gain <= 0.0
        || lambda < 0.0
        || saturation_current <= 0.0
        || emission <= 0.0
    {
        return None;
    }
    let swapped = params
        .get("swap_ds")
        .and_then(ParamValue::as_bool)
        .unwrap_or(false);
    // Source pin at 0 V, drain pin at `vds`, gate pin at `vgs`.
    let (internal_drain, internal_source) = if swapped { (0.0, vds) } else { (vds, 0.0) };
    let gate_source = polarity * (vgs - internal_source);
    let drain_source = polarity * (internal_drain - internal_source);
    let forward = |gate_source: f64, drain_source: f64| -> f64 {
        let overdrive = gate_source - threshold;
        if overdrive <= 0.0 {
            return 0.0;
        }
        let modulation = 1.0 + lambda * drain_source;
        if drain_source < overdrive {
            gain * (2.0 * overdrive * drain_source - drain_source * drain_source) * modulation
        } else {
            gain * overdrive * overdrive * modulation
        }
    };
    let channel_current = if drain_source >= 0.0 {
        forward(gate_source, drain_source)
    } else {
        -forward(gate_source - drain_source, -drain_source)
    };
    let thermal = emission * 1.380_649e-23 * 300.15 / 1.602_176_634e-19;
    let body = saturation_current * (((-drain_source) / thermal).min(80.0)).exp_m1();
    let internal = polarity * (channel_current - body);
    // With permuted terminals the symbol's drain is the internal source.
    Some(if swapped { -internal } else { internal })
}

fn comparator_sink_current(
    params: &sim_core::netlist::ParamMap,
    input_difference: f64,
    output_voltage: f64,
) -> Option<f64> {
    let number = |key: &str| {
        params
            .get(key)
            .and_then(ParamValue::as_number)
            .filter(|value| value.is_finite())
    };
    let offset = number("v_offset")?;
    let resistance = number("r_sink")?;
    let maximum_current = number("imax")?;
    if offset.abs() > 5e-3 || resistance <= 0.0 || maximum_current <= 0.0 || maximum_current > 20e-3
    {
        return None;
    }
    if input_difference < offset {
        Some((output_voltage / resistance).min(maximum_current))
    } else {
        Some(0.0)
    }
}

fn diode_params_current(
    params: &sim_core::netlist::ParamMap,
    zener: bool,
    voltage: f64,
) -> Option<f64> {
    const THERMAL_VOLTAGE: f64 = 0.025_864_925_786_329;
    let number = |key: &str| {
        params
            .get(key)
            .and_then(ParamValue::as_number)
            .filter(|value| value.is_finite())
    };
    let emission = number("n").unwrap_or(2.0);
    if emission <= 0.0 {
        return None;
    }
    let saturation = number("is").or_else(|| {
        let forward_voltage = number("vf")?;
        let forward_current = number("if")?;
        let denominator = (forward_voltage / (emission * THERMAL_VOLTAGE))
            .min(80.0)
            .exp()
            - 1.0;
        (denominator > 0.0).then_some(forward_current / denominator)
    })?;
    let forward = (voltage / (emission * THERMAL_VOLTAGE)).min(80.0).exp();
    let mut current = saturation * (forward - 1.0);
    if zener {
        let breakdown = number("bv")?;
        let breakdown_saturation = number("ibv")?;
        let breakdown_emission = number("nbv")?;
        if breakdown_emission <= 0.0 {
            return None;
        }
        let reverse = (-(voltage + breakdown) / (breakdown_emission * THERMAL_VOLTAGE))
            .min(80.0)
            .exp();
        current -= breakdown_saturation * (reverse - 1.0);
    }
    Some(current)
}

/// Simulation error rendered in French by the interface.
#[derive(Debug, Error)]
pub enum SimulationError {
    /// The worker could not be configured.
    #[error("configuration du worker invalide : {0}")]
    Worker(#[from] worker::WorkerError),
    /// Frame encoding or decoding failed.
    #[error("le protocole de calcul a échoué : {0}")]
    Protocol(#[from] worker::protocol::ProtocolError),
    /// The worker adapter failed.
    #[error("le service de calcul a échoué : {0}")]
    Service(#[from] worker::ServiceError),
    /// The worker refused a request.
    #[error("le worker a refusé la requête : {0}")]
    Rejected(String),
    /// The solver reported an analysis failure.
    #[error("l'analyse {analysis} a échoué : {message}")]
    AnalysisFailed {
        /// French analysis name.
        analysis: &'static str,
        /// Technical solver diagnostic.
        message: String,
    },
    /// A field contains invalid engineering text.
    #[error("le champ « {field} » est invalide : {message}")]
    InvalidField {
        /// French field name.
        field: &'static str,
        /// Technical parser detail.
        message: String,
    },
    /// A transient interval has no positive duration.
    #[error("l'instant d'arrêt doit être strictement postérieur à l'instant de début")]
    EmptyDuration,
    /// A transient interval is too fine for interactive use.
    #[error("le transitoire dépasse la limite interactive de {limit} pas")]
    TooManySteps {
        /// Maximum accepted number of intervals.
        limit: usize,
    },
    /// The AC sweep point count is outside the supported range.
    #[error("le nombre de points AC doit être compris entre 2 et {max}")]
    InvalidPointCount {
        /// Maximum accepted number of points.
        max: usize,
    },
    /// The selected source disappeared from the netlist.
    #[error("la source « {0} » n'existe plus dans la netlist")]
    MissingSource(String),
    /// The real-time transient session failed.
    #[error("simulation transitoire : {0}")]
    Transient(String),
    /// The selected signal or net disappeared from the netlist.
    #[error("le signal « {0} » n'existe plus dans le résultat")]
    MissingSignal(String),
    /// A source-step was requested without a selected source.
    #[error("sélectionnez une source pour l'échelon")]
    MissingStepSource,
    /// The source-step target is not an independent source.
    #[error("le composant « {0} » n'est pas une source indépendante")]
    NotSource(String),
    /// The netlist could not be rebuilt after a source override.
    #[error("la netlist préparée est invalide : {0}")]
    Netlist(#[from] NetlistError),
    /// No trace can be produced from the current data.
    #[error("aucune donnée exploitable n'est disponible")]
    NoData,
    /// The FFT needs enough uniform samples.
    #[error("le spectre exige au moins 16 échantillons")]
    SpectrumTooShort,
    /// The FFT step is not usable.
    #[error("le pas du transitoire est invalide pour le spectre")]
    InvalidSpectrumStep,
    /// A Bode trace requires a voltage output.
    #[error("le diagramme de Bode exige une tension de sortie")]
    BodeRequiresVoltage,
    /// A worker result belongs to an unexpected request.
    #[error("le worker a renvoyé une réponse inattendue")]
    UnexpectedResponse,
    /// One analysis is already pending.
    #[error("une analyse est déjà en cours")]
    Busy,
    /// The internal revision counter is exhausted.
    #[error("le compteur de révisions du worker est épuisé")]
    RevisionExhausted,
}

impl SimulationError {
    /// Returns a concise French message suitable for the interface.
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

/// Builds a fixed-step transient request, including an optional source step.
pub fn prepare_transient(
    extraction: &Extraction,
    settings: &TransientSettings,
    document_revision: u64,
) -> Result<PreparedAnalysis, SimulationError> {
    let start_time = parse_time("temps de début", &settings.start_time)?;
    let stop_time = parse_time("temps d'arrêt", &settings.stop_time)?;
    let step = parse_time("pas de calcul", &settings.step)?;
    if stop_time <= start_time {
        return Err(SimulationError::EmptyDuration);
    }
    if step <= 0.0 {
        return Err(SimulationError::InvalidField {
            field: "pas de calcul",
            message: "le pas doit être strictement positif".to_owned(),
        });
    }
    let intervals = (stop_time - start_time) / step;
    if !intervals.is_finite() || intervals > MAX_TRANSIENT_STEPS as f64 {
        return Err(SimulationError::TooManySteps {
            limit: MAX_TRANSIENT_STEPS,
        });
    }

    let mut netlist = extraction.netlist.clone();
    let mut events = Vec::new();
    if settings.step_enabled {
        let label = settings
            .source
            .as_ref()
            .ok_or(SimulationError::MissingStepSource)?;
        let component_id = {
            let component = netlist
                .components
                .iter()
                .find(|component| &component.label == label)
                .ok_or_else(|| SimulationError::MissingSource(label.clone()))?;
            if !matches!(
                component.kind,
                ComponentKind::VoltageSource | ComponentKind::CurrentSource
            ) {
                return Err(SimulationError::NotSource(label.clone()));
            }
            component.id
        };
        let initial_value = parse_number("valeur initiale", &settings.initial_value)?;
        let final_value = parse_number("valeur finale", &settings.final_value)?;
        let event_time = parse_time("instant de l'échelon", &settings.event_time)?;
        if event_time < start_time || event_time >= stop_time {
            return Err(SimulationError::InvalidField {
                field: "instant de l'échelon",
                message: "l'instant doit appartenir à [début, arrêt)".to_owned(),
            });
        }
        if let Some(target) = netlist
            .components
            .iter_mut()
            .find(|component| component.id == component_id)
        {
            target
                .params
                .insert("dc".to_owned(), ParamValue::Number(initial_value));
        }
        events.push(TransientEvent {
            time: event_time,
            action: TransientAction::SetSourceValue {
                component: component_id,
                value: final_value,
            },
        });
        netlist.validate()?;
    }

    let interval_count = intervals.ceil() as usize;
    let max_steps = interval_count
        .saturating_add(events.len())
        .saturating_add(4);
    let analysis = Analysis::Transient(TransientOptions {
        start_time,
        stop_time,
        step,
        max_steps,
        solver: DcOptions::default(),
        events,
    });
    let context = RunContext {
        document_revision,
        nodes: node_map(&netlist),
        components: component_map(&netlist),
        input_amplitude: None,
        nominal_step: Some(step),
    };
    Ok(PreparedAnalysis {
        netlist,
        analysis,
        context,
    })
}

/// Builds a logarithmic small-signal AC request for one source.
pub fn prepare_ac(
    extraction: &Extraction,
    settings: &AcSettings,
    document_revision: u64,
) -> Result<PreparedAnalysis, SimulationError> {
    if !(2..=MAX_AC_POINTS).contains(&settings.points) {
        return Err(SimulationError::InvalidPointCount { max: MAX_AC_POINTS });
    }
    let start = parse_frequency("fréquence de début", &settings.start)?;
    let stop = parse_frequency("fréquence de fin", &settings.stop)?;
    if start <= 0.0 || stop <= 0.0 {
        return Err(SimulationError::InvalidField {
            field: "fréquence",
            message: "les fréquences doivent être strictement positives".to_owned(),
        });
    }
    if stop <= start {
        return Err(SimulationError::InvalidField {
            field: "fréquence de fin",
            message: "la borne haute doit dépasser la borne basse".to_owned(),
        });
    }
    let amplitude = parse_number("amplitude d'excitation", &settings.amplitude)?;
    if amplitude <= 0.0 {
        return Err(SimulationError::InvalidField {
            field: "amplitude d'excitation",
            message: "l'amplitude doit être strictement positive".to_owned(),
        });
    }
    let label = settings
        .source
        .as_ref()
        .ok_or_else(|| SimulationError::MissingSource("source AC".to_owned()))?;
    let component = extraction
        .netlist
        .components
        .iter()
        .find(|component| &component.label == label)
        .ok_or_else(|| SimulationError::MissingSource(label.clone()))?;
    if !matches!(
        component.kind,
        ComponentKind::VoltageSource | ComponentKind::CurrentSource
    ) {
        return Err(SimulationError::NotSource(label.clone()));
    }
    let analysis = Analysis::Ac(AcOptions {
        sweep: AcSweep::Logarithmic {
            start,
            stop,
            points: settings.points,
        },
        excitations: vec![AcExcitation {
            component: component.id,
            magnitude: amplitude,
            phase_degrees: 0.0,
        }],
        solver: DcOptions::default(),
    });
    let context = RunContext {
        document_revision,
        nodes: node_map(&extraction.netlist),
        components: component_map(&extraction.netlist),
        input_amplitude: Some(amplitude),
        nominal_step: None,
    };
    Ok(PreparedAnalysis {
        netlist: extraction.netlist.clone(),
        analysis,
        context,
    })
}

fn parse_time(field: &'static str, text: &str) -> Result<f64, SimulationError> {
    let value = parse_si(text).map_err(|error| SimulationError::InvalidField {
        field,
        message: error.to_string(),
    })?;
    if !value.is_finite() {
        return Err(SimulationError::InvalidField {
            field,
            message: "la valeur doit être finie".to_owned(),
        });
    }
    Ok(value)
}

/// Parses the integration step of the live session.
///
/// Unlike the analysis step it is entered on its own, so the error names the
/// live field and a non-positive value is rejected instead of silently replaced.
pub fn parse_live_step(text: &str) -> Result<f64, SimulationError> {
    let step = parse_time("pas temps réel", text)?;
    if step <= 0.0 {
        return Err(SimulationError::InvalidField {
            field: "pas temps réel",
            message: "le pas doit être strictement positif".to_owned(),
        });
    }
    Ok(step)
}

fn parse_frequency(field: &'static str, text: &str) -> Result<f64, SimulationError> {
    parse_time(field, text)
}

fn parse_number(field: &'static str, text: &str) -> Result<f64, SimulationError> {
    let value = parse_si(text).map_err(|error| SimulationError::InvalidField {
        field,
        message: error.to_string(),
    })?;
    if !value.is_finite() {
        return Err(SimulationError::InvalidField {
            field,
            message: "la valeur doit être finie".to_owned(),
        });
    }
    Ok(value)
}

fn node_map(netlist: &Netlist) -> BTreeMap<String, NodeId> {
    netlist
        .nodes
        .iter()
        .map(|node| (node.name.as_str().to_owned(), node.id))
        .collect()
}

fn component_map(netlist: &Netlist) -> BTreeMap<String, ComponentId> {
    netlist
        .components
        .iter()
        .map(|component| (component.label.clone(), component.id))
        .collect()
}

/// Frame-oriented client with one pending job and monotonic revisions.
pub struct SimulationClient {
    service: WorkerService,
    next_revision: u64,
    pending: Option<PendingJob>,
}

struct PendingJob {
    revision: u64,
    kind: AnalysisKind,
    context: RunContext,
}

impl SimulationClient {
    /// Creates a client with the default worker rate limit.
    pub fn new() -> Result<Self, SimulationError> {
        Ok(Self {
            service: WorkerService::new(WorkerConfig::default())?,
            next_revision: 1,
            pending: None,
        })
    }

    /// Returns true when the worker still holds a submitted job.
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Returns the analysis family of the pending job.
    pub fn pending_kind(&self) -> Option<AnalysisKind> {
        self.pending.as_ref().map(|pending| pending.kind)
    }

    /// Submits a prepared analysis and stores it in the worker slot.
    pub fn submit(&mut self, prepared: PreparedAnalysis) -> Result<u64, SimulationError> {
        if self.pending.is_some() {
            return Err(SimulationError::Busy);
        }
        if self.next_revision == 0 || self.next_revision == u64::MAX {
            return Err(SimulationError::RevisionExhausted);
        }
        let revision = self.next_revision;
        let PreparedAnalysis {
            netlist,
            analysis,
            context,
        } = prepared;
        let kind = analysis.kind();
        let frame = encode_request(&ClientMessage::Submit {
            job: Job {
                revision,
                netlist,
                analysis,
            },
        })?;
        let response = decode_response(&self.service.submit_frame(&frame)?)?;
        match response {
            ServerMessage::Accepted { revision: accepted }
            | ServerMessage::Coalesced {
                revision: accepted, ..
            } if accepted == revision => {
                self.next_revision = revision
                    .checked_add(1)
                    .ok_or(SimulationError::RevisionExhausted)?;
                self.pending = Some(PendingJob {
                    revision,
                    kind,
                    context,
                });
                Ok(revision)
            }
            ServerMessage::Rejected { reason, .. } => {
                Err(SimulationError::Rejected(rejection_message(reason)))
            }
            _ => Err(SimulationError::UnexpectedResponse),
        }
    }

    /// Polls the worker once and returns a completed analysis when available.
    pub fn poll(&mut self, now_seconds: f64) -> Result<Option<CompletedRun>, SimulationError> {
        let Some(frame) = self.service.poll_frame(now_seconds)? else {
            return Ok(None);
        };
        let pending = self
            .pending
            .take()
            .ok_or(SimulationError::UnexpectedResponse)?;
        let response = decode_response(&frame)?;
        match response {
            ServerMessage::Result { revision, result } if revision == pending.revision => {
                Ok(Some(CompletedRun {
                    revision,
                    kind: pending.kind,
                    result,
                    context: pending.context,
                }))
            }
            ServerMessage::Failed {
                revision, failure, ..
            } if revision == pending.revision => Err(SimulationError::AnalysisFailed {
                analysis: failure_kind_name(failure.kind),
                message: failure.message,
            }),
            _ => Err(SimulationError::UnexpectedResponse),
        }
    }
}

fn rejection_message(reason: RejectionReason) -> String {
    match reason {
        RejectionReason::InvalidRevision => "révision nulle interdite".to_owned(),
        RejectionReason::StaleRevision { latest } => {
            format!("révision obsolète (dernière révision : {latest})")
        }
    }
}

/// Returns the French name of an analysis family.
pub const fn analysis_name(kind: AnalysisKind) -> &'static str {
    match kind {
        AnalysisKind::Dc => "DC",
        AnalysisKind::Transient => "transitoire",
        AnalysisKind::Ac => "AC",
    }
}

fn failure_kind_name(kind: AnalysisFailureKind) -> &'static str {
    match kind {
        AnalysisFailureKind::Netlist => "netlist",
        AnalysisFailureKind::Dc => "point de fonctionnement",
        AnalysisFailureKind::Transient => "transitoire",
        AnalysisFailureKind::Ac => "AC",
    }
}

/// One sampled waveform.
#[derive(Clone, Debug, PartialEq)]
pub struct Trace {
    /// Signal represented by the samples.
    pub signal: SignalId,
    /// Samples ordered by increasing time.
    pub points: Vec<[f64; 2]>,
}

impl Trace {
    /// Returns the label displayed in plots.
    pub fn label(&self) -> String {
        self.signal.label()
    }

    /// Returns the waveform unit.
    pub const fn unit(&self) -> SignalUnit {
        self.signal.unit()
    }
}

/// Prepared real-time transient session.
#[derive(Debug)]
pub struct RealtimeSetup {
    /// Netlist snapshot with the source step applied.
    pub netlist: Netlist,
    /// Name and revision mapping.
    pub context: RunContext,
    /// Fixed-step options with an open horizon.
    pub options: TransientOptions,
}

/// Builds an open-ended transient session from the current settings.
pub fn prepare_realtime(
    extraction: &Extraction,
    settings: &TransientSettings,
    document_revision: u64,
) -> Result<RealtimeSetup, SimulationError> {
    let step = parse_time("pas de calcul", &settings.step)?;
    if step <= 0.0 {
        return Err(SimulationError::InvalidField {
            field: "pas de calcul",
            message: "le pas doit être strictement positif".to_owned(),
        });
    }
    let mut netlist = extraction.netlist.clone();
    let mut events = Vec::new();
    if settings.step_enabled {
        let label = settings
            .source
            .as_ref()
            .ok_or(SimulationError::MissingStepSource)?;
        let component_id = {
            let component = netlist
                .components
                .iter()
                .find(|component| &component.label == label)
                .ok_or_else(|| SimulationError::MissingSource(label.clone()))?;
            if !matches!(
                component.kind,
                ComponentKind::VoltageSource | ComponentKind::CurrentSource
            ) {
                return Err(SimulationError::NotSource(label.clone()));
            }
            component.id
        };
        let initial_value = parse_number("valeur initiale", &settings.initial_value)?;
        let final_value = parse_number("valeur finale", &settings.final_value)?;
        let event_time = parse_time("instant de l'échelon", &settings.event_time)?;
        if event_time < 0.0 {
            return Err(SimulationError::InvalidField {
                field: "instant de l'échelon",
                message: "l'instant doit être positif ou nul".to_owned(),
            });
        }
        if let Some(target) = netlist
            .components
            .iter_mut()
            .find(|component| component.id == component_id)
        {
            target
                .params
                .insert("dc".to_owned(), ParamValue::Number(initial_value));
        }
        events.push(TransientEvent {
            time: event_time,
            action: TransientAction::SetSourceValue {
                component: component_id,
                value: final_value,
            },
        });
        netlist.validate()?;
    }
    let context = RunContext {
        document_revision,
        nodes: node_map(&netlist),
        components: component_map(&netlist),
        input_amplitude: None,
        nominal_step: Some(step),
    };
    Ok(RealtimeSetup {
        netlist,
        context,
        options: TransientOptions {
            start_time: 0.0,
            stop_time: f64::MAX,
            step,
            max_steps: MAX_TRANSIENT_STEPS,
            solver: DcOptions::default(),
            events,
        },
    })
}

/// Lists capacitors whose voltage can be plotted by the scopes.
pub fn capacitor_labels(document: &crate::document::Document) -> Vec<String> {
    let mut labels = document
        .schematic()
        .symbols()
        .filter(|symbol| symbol.kind == SymbolKind::Capacitor)
        .map(|symbol| symbol.label.clone())
        .collect::<Vec<_>>();
    labels.sort();
    labels.dedup();
    labels
}

/// Lists components whose current can be plotted by the scopes.
pub fn component_current_labels(document: &crate::document::Document) -> Vec<String> {
    let mut labels = document
        .schematic()
        .symbols()
        .filter(|symbol| {
            !symbol.kind.is_measurement()
                && !symbol.kind.is_ground()
                && !matches!(symbol.kind, SymbolKind::Custom(_))
        })
        .map(|symbol| symbol.label.clone())
        .collect::<Vec<_>>();
    labels.sort();
    labels.dedup();
    labels
}

/// Extracts traces from an arbitrary sample buffer, deriving net currents
/// that the solver does not expose as branch variables.
pub fn transient_traces_from_samples_with_document(
    samples: &[TransientSample],
    context: &RunContext,
    document: &crate::document::Document,
    signals: &[SignalId],
) -> Result<Vec<Trace>, SimulationError> {
    let extraction = document.extraction();
    let mut traces = Vec::with_capacity(signals.len());
    for signal in signals {
        let mut points = Vec::with_capacity(samples.len());
        match signal {
            SignalId::Voltage(name) => {
                let node = context
                    .nodes
                    .get(name)
                    .copied()
                    .ok_or_else(|| SimulationError::MissingSignal(signal.label()))?;
                for sample in samples {
                    if let Some(value) = sample.node_voltage(node) {
                        points.push([sample.time, value]);
                    }
                }
            }
            SignalId::ComponentVoltage(name) => {
                let Some(extraction) = extraction else {
                    return Err(SimulationError::MissingSignal(signal.label()));
                };
                let spec = extraction
                    .netlist
                    .components
                    .iter()
                    .find(|spec| spec.label == *name)
                    .ok_or_else(|| SimulationError::MissingSignal(signal.label()))?;
                let (positive_pin, negative_pin) = match spec.kind {
                    ComponentKind::ShuntReference => (spec.pins.get(1), spec.pins.get(2)),
                    ComponentKind::Comparator => (spec.pins.get(2), spec.pins.get(4)),
                    _ => (spec.pins.first(), spec.pins.get(1)),
                };
                let (Some(positive_pin), Some(negative_pin)) = (positive_pin, negative_pin) else {
                    return Err(SimulationError::MissingSignal(signal.label()));
                };
                for sample in samples {
                    if let (Some(a), Some(b)) = (
                        sample.node_voltage(*positive_pin),
                        sample.node_voltage(*negative_pin),
                    ) {
                        points.push([sample.time, a - b]);
                    }
                }
            }
            SignalId::NetCurrent(name) => {
                let Some(extraction) = extraction else {
                    return Err(SimulationError::MissingSignal(signal.label()));
                };
                for (index, sample) in samples.iter().enumerate() {
                    let previous = index.checked_sub(1).and_then(|index| samples.get(index));
                    if let Some(value) =
                        sample_net_current(extraction, document, sample, previous, name)
                    {
                        points.push([sample.time, value]);
                    }
                }
            }
            SignalId::Current(name) => {
                let component = context
                    .components
                    .get(name)
                    .copied()
                    .ok_or_else(|| SimulationError::MissingSignal(signal.label()))?;
                let spec = extraction.and_then(|extraction| {
                    extraction
                        .netlist
                        .components
                        .iter()
                        .find(|spec| spec.id == component)
                });
                for (index, sample) in samples.iter().enumerate() {
                    let previous = index.checked_sub(1).and_then(|index| samples.get(index));
                    let value = sample.branch_current(component).or_else(|| {
                        spec.and_then(|spec| derived_component_current(spec, sample, previous))
                    });
                    if let Some(value) = value {
                        points.push([sample.time, value]);
                    }
                }
            }
        }
        let floor = trace_floor(signal);
        for point in &mut points {
            if point[1].abs() < floor {
                point[1] = 0.0;
            }
        }
        if points.is_empty() {
            return Err(SimulationError::NoData);
        }
        traces.push(Trace {
            signal: signal.clone(),
            points,
        });
    }
    Ok(traces)
}

fn sample_net_current(
    extraction: &Extraction,
    document: &crate::document::Document,
    sample: &TransientSample,
    previous: Option<&TransientSample>,
    net_name: &str,
) -> Option<f64> {
    if !extraction
        .nets
        .iter()
        .any(|net| net.name.as_str() == net_name)
    {
        return None;
    }
    let mut strength = 0.0_f64;
    for symbol in document.schematic().symbols() {
        if symbol.kind.is_measurement() || symbol.kind.is_ground() {
            continue;
        }
        let Some(definition) = document.registry().get(&symbol.kind) else {
            continue;
        };
        if definition.pins.len() != 2
            && !matches!(
                symbol.kind,
                SymbolKind::ShuntReference | SymbolKind::Comparator
            )
        {
            continue;
        }
        let Some(spec) = extraction
            .netlist
            .components
            .iter()
            .find(|spec| spec.label == symbol.label)
        else {
            continue;
        };
        if spec.kind == ComponentKind::Switch
            && !spec
                .params
                .get("closed")
                .and_then(ParamValue::as_bool)
                .unwrap_or(false)
        {
            continue;
        }
        let current = sample
            .branch_current(spec.id)
            .or_else(|| derived_component_current(spec, sample, previous));
        let Some(current) = current.filter(|current| current.is_finite()) else {
            continue;
        };
        let injections = symbol_terminal_injections(symbol, current);
        for (index, injection) in injections
            .iter()
            .copied()
            .enumerate()
            .take(definition.pins.len())
        {
            let Some(&node) = spec.pins.get(index) else {
                continue;
            };
            let Some(net) = extraction.nets.iter().find(|net| net.node == node) else {
                continue;
            };
            if net.name.as_str() == net_name {
                strength = strength.max(injection.abs());
            }
        }
    }
    Some(strength)
}

fn derived_component_current(
    spec: &ComponentSpec,
    sample: &TransientSample,
    previous: Option<&TransientSample>,
) -> Option<f64> {
    if spec.kind == ComponentKind::Comparator {
        let input_plus = sample.node_voltage(*spec.pins.first()?)?;
        let input_minus = sample.node_voltage(*spec.pins.get(1)?)?;
        let output = sample.node_voltage(*spec.pins.get(2)?)?;
        let positive_supply = sample.node_voltage(*spec.pins.get(3)?)?;
        let negative_supply = sample.node_voltage(*spec.pins.get(4)?)?;
        return comparator_sink_current(
            &spec.params,
            if (2.0..=36.0).contains(&(positive_supply - negative_supply)) {
                input_plus - input_minus
            } else {
                1.0
            },
            output - negative_supply,
        );
    }
    if spec.kind == ComponentKind::ShuntReference {
        let reference = sample.node_voltage(*spec.pins.first()?)?;
        let cathode = sample.node_voltage(*spec.pins.get(1)?)?;
        let anode = sample.node_voltage(*spec.pins.get(2)?)?;
        return shunt_reference_current(&spec.params, reference - anode, cathode - anode);
    }
    if spec.pins.len() != 2 {
        return None;
    }
    let a = sample.node_voltage(spec.pins[0])?;
    let b = sample.node_voltage(spec.pins[1])?;
    let voltage = a - b;
    match spec.kind {
        ComponentKind::Resistor => {
            let resistance = component_number(spec, "resistance")?;
            (resistance.abs() > f64::MIN_POSITIVE).then_some(voltage / resistance)
        }
        ComponentKind::Switch => {
            let closed = spec
                .params
                .get("closed")
                .and_then(ParamValue::as_bool)
                .unwrap_or(false);
            if !closed {
                return Some(0.0);
            }
            let resistance = component_number(spec, "ron")?;
            (resistance.abs() > f64::MIN_POSITIVE).then_some(voltage / resistance)
        }
        ComponentKind::Capacitor => {
            let capacitance = component_number(spec, "capacitance")?;
            let previous = previous?;
            let dt = sample.time - previous.time;
            if dt <= 0.0 {
                return None;
            }
            let previous_voltage =
                previous.node_voltage(spec.pins[0])? - previous.node_voltage(spec.pins[1])?;
            Some(capacitance * (voltage - previous_voltage) / dt)
        }
        ComponentKind::CurrentSource => component_number(spec, "dc"),
        ComponentKind::Diode | ComponentKind::Led | ComponentKind::Zener => {
            diode_current(spec, voltage)
        }
        _ => None,
    }
}

fn diode_current(spec: &ComponentSpec, voltage: f64) -> Option<f64> {
    diode_params_current(&spec.params, spec.kind == ComponentKind::Zener, voltage)
}

fn component_number(spec: &ComponentSpec, key: &str) -> Option<f64> {
    spec.params
        .get(key)
        .and_then(ParamValue::as_number)
        .filter(|value| value.is_finite())
}

/// Extracts traces from an arbitrary sample buffer.
pub fn transient_traces_from_samples(
    samples: &[TransientSample],
    context: &RunContext,
    signals: &[SignalId],
) -> Result<Vec<Trace>, SimulationError> {
    let mut traces = Vec::with_capacity(signals.len());
    for signal in signals {
        let mut points = Vec::with_capacity(samples.len());
        match signal {
            SignalId::Voltage(name) => {
                let node = context
                    .nodes
                    .get(name)
                    .copied()
                    .ok_or_else(|| SimulationError::MissingSignal(signal.label()))?;
                for sample in samples {
                    if let Some(value) = sample.node_voltage(node) {
                        points.push([sample.time, value]);
                    }
                }
            }
            SignalId::ComponentVoltage(name) => {
                return Err(SimulationError::MissingSignal(format!("U({name})")));
            }
            SignalId::NetCurrent(name) => {
                return Err(SimulationError::MissingSignal(format!("I({name})")));
            }
            SignalId::Current(name) => {
                let component = context
                    .components
                    .get(name)
                    .copied()
                    .ok_or_else(|| SimulationError::MissingSignal(signal.label()))?;
                for sample in samples {
                    if let Some(value) = sample.branch_current(component) {
                        points.push([sample.time, value]);
                    }
                }
            }
        }
        let floor = trace_floor(signal);
        for point in &mut points {
            if point[1].abs() < floor {
                point[1] = 0.0;
            }
        }
        if points.is_empty() {
            return Err(SimulationError::NoData);
        }
        traces.push(Trace {
            signal: signal.clone(),
            points,
        });
    }
    Ok(traces)
}

/// Extracts the requested transient signals from one worker result.
pub fn transient_traces(
    result: &TransientResult,
    context: &RunContext,
    signals: &[SignalId],
) -> Result<Vec<Trace>, SimulationError> {
    transient_traces_from_samples(&result.samples, context, signals)
}

/// Summary values computed from one waveform.
#[derive(Clone, Debug, PartialEq)]
pub struct Measurement {
    /// Measured signal.
    pub signal: SignalId,
    /// Smallest sample.
    pub minimum: f64,
    /// Largest sample.
    pub maximum: f64,
    /// Peak-to-peak amplitude.
    pub peak_to_peak: f64,
    /// Arithmetic mean.
    pub mean: f64,
    /// Root mean square.
    pub rms: f64,
    /// Number of samples.
    pub samples: usize,
}

/// Computes minimum, maximum, mean, RMS, and peak-to-peak values.
pub fn measurements(traces: &[Trace]) -> Vec<Measurement> {
    traces
        .iter()
        .filter_map(|trace| {
            let first = trace.points.first()?;
            let mut minimum = first[1];
            let mut maximum = first[1];
            let mut sum = 0.0;
            let mut square_sum = 0.0;
            for point in &trace.points {
                minimum = minimum.min(point[1]);
                maximum = maximum.max(point[1]);
                sum += point[1];
                square_sum += point[1] * point[1];
            }
            let samples = trace.points.len();
            let count = samples as f64;
            Some(Measurement {
                signal: trace.signal.clone(),
                minimum,
                maximum,
                peak_to_peak: maximum - minimum,
                mean: sum / count,
                rms: (square_sum / count).sqrt(),
                samples,
            })
        })
        .collect()
}

/// One frequency-domain point of an AC response.
#[derive(Clone, Debug, PartialEq)]
pub struct BodePoint {
    /// Frequency in hertz.
    pub frequency: f64,
    /// Output magnitude in volts.
    pub magnitude: f64,
    /// Gain relative to the excitation amplitude, in decibels.
    pub gain_db: f64,
    /// Output phase in degrees.
    pub phase_degrees: f64,
}

/// Gain and phase response for one voltage net.
#[derive(Clone, Debug, PartialEq)]
pub struct BodeTrace {
    /// Measured net voltage.
    pub signal: SignalId,
    /// Points ordered by increasing frequency.
    pub points: Vec<BodePoint>,
}

/// Extracts one voltage Bode response from an AC result.
pub fn bode_trace(
    result: &AcResult,
    context: &RunContext,
    signal: &SignalId,
) -> Result<BodeTrace, SimulationError> {
    let SignalId::Voltage(name) = signal else {
        return Err(SimulationError::BodeRequiresVoltage);
    };
    let node = context
        .nodes
        .get(name)
        .copied()
        .ok_or_else(|| SimulationError::MissingSignal(signal.label()))?;
    let amplitude = context
        .input_amplitude
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or(SimulationError::NoData)?;
    let mut points = Vec::with_capacity(result.samples.len());
    for sample in &result.samples {
        let Some(phasor) = sample.node_voltage(node) else {
            continue;
        };
        let magnitude = phasor.magnitude();
        if !magnitude.is_finite() || magnitude <= 0.0 {
            continue;
        }
        let gain_db = 20.0 * (magnitude / amplitude).log10();
        let phase_degrees = phasor.phase_degrees();
        if !gain_db.is_finite() || !phase_degrees.is_finite() {
            continue;
        }
        points.push(BodePoint {
            frequency: sample.frequency,
            magnitude,
            gain_db,
            phase_degrees,
        });
    }
    if points.is_empty() {
        return Err(SimulationError::NoData);
    }
    Ok(BodeTrace {
        signal: signal.clone(),
        points,
    })
}

/// One frequency-domain amplitude sample.
#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    /// Frequency of each one-sided bin in hertz.
    pub frequencies: Vec<f64>,
    /// Peak amplitude of each bin in signal units.
    pub magnitudes: Vec<f64>,
    /// Frequency spacing in hertz.
    pub resolution: f64,
    /// Number of FFT input samples.
    pub sample_count: usize,
    /// Strongest non-DC bin.
    pub peak: Option<SpectrumPeak>,
}

/// Strongest non-DC spectrum component.
#[derive(Clone, Debug, PartialEq)]
pub struct SpectrumPeak {
    /// Frequency in hertz.
    pub frequency: f64,
    /// Amplitude in signal units.
    pub magnitude: f64,
}

/// Computes a Hann-windowed, mean-removed one-sided amplitude spectrum.
pub fn compute_spectrum(trace: &Trace, nominal_step: f64) -> Result<Spectrum, SimulationError> {
    if trace.points.len() < 16 {
        return Err(SimulationError::SpectrumTooShort);
    }
    if !nominal_step.is_finite() || nominal_step <= 0.0 {
        return Err(SimulationError::InvalidSpectrumStep);
    }
    let start = trace.points[0][0];
    let end = trace.points[trace.points.len() - 1][0];
    let duration = end - start;
    if !duration.is_finite() || duration <= 0.0 {
        return Err(SimulationError::NoData);
    }
    let available = ((duration / nominal_step).floor() as usize).clamp(16, MAX_FFT_POINTS);
    let sample_count = available.next_power_of_two().min(MAX_FFT_POINTS);
    let dt = duration / sample_count as f64;
    if !dt.is_finite() || dt <= 0.0 {
        return Err(SimulationError::InvalidSpectrumStep);
    }

    let mut values = Vec::with_capacity(sample_count);
    for index in 0..sample_count {
        let time = start + index as f64 * dt;
        values.push(interpolate(&trace.points, time));
    }
    let mean = values.iter().sum::<f64>() / sample_count as f64;
    let mut window_sum = 0.0;
    let mut buffer = Vec::with_capacity(sample_count);
    for (index, value) in values.into_iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / sample_count as f64).cos();
        window_sum += window;
        buffer.push(Complex::new((value - mean) * window, 0.0));
    }
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(sample_count);
    fft.process(&mut buffer);

    let half = sample_count / 2;
    let mut frequencies = Vec::with_capacity(half + 1);
    let mut magnitudes = Vec::with_capacity(half + 1);
    for (index, bin) in buffer.iter().enumerate().take(half + 1) {
        let frequency = index as f64 / duration;
        let factor = if index == 0 || (sample_count.is_multiple_of(2) && index == half) {
            1.0
        } else {
            2.0
        };
        let magnitude = factor * bin.norm() / window_sum;
        frequencies.push(frequency);
        magnitudes.push(magnitude);
    }

    let mut peak_index = 0;
    let mut peak_magnitude = 0.0;
    for (index, magnitude) in magnitudes.iter().enumerate().skip(1) {
        if *magnitude > peak_magnitude {
            peak_index = index;
            peak_magnitude = *magnitude;
        }
    }
    let peak = if peak_index == 0 || peak_magnitude <= 0.0 {
        None
    } else {
        Some(SpectrumPeak {
            frequency: frequencies[peak_index],
            magnitude: peak_magnitude,
        })
    };
    Ok(Spectrum {
        frequencies,
        magnitudes,
        resolution: 1.0 / duration,
        sample_count,
        peak,
    })
}

fn interpolate(points: &[[f64; 2]], time: f64) -> f64 {
    let index = points.partition_point(|point| point[0] <= time);
    if index == 0 {
        return points[0][1];
    }
    if index >= points.len() {
        return points[points.len() - 1][1];
    }
    let left = points[index - 1];
    let right = points[index];
    let span = right[0] - left[0];
    if span <= 0.0 {
        left[1]
    } else {
        let ratio = (time - left[0]) / span;
        left[1] + (right[1] - left[1]) * ratio
    }
}

/// Serializes transient traces as tidy long-form CSV.
pub fn transient_csv(traces: &[Trace]) -> String {
    let mut output = String::from("signal,unit,time_s,value\n");
    for trace in traces {
        let label = csv_field(&trace.label());
        let unit = trace.unit().symbol();
        for point in &trace.points {
            push_csv_row(
                &mut output,
                &[
                    label.clone(),
                    unit.to_owned(),
                    csv_number(point[0]),
                    csv_number(point[1]),
                ],
            );
        }
    }
    output
}

/// Serializes one amplitude spectrum as CSV.
pub fn spectrum_csv(trace: &Trace, spectrum: &Spectrum) -> String {
    let mut output = String::from("signal,unit,frequency_hz,magnitude\n");
    let label = csv_field(&trace.label());
    let unit = trace.unit().symbol();
    for (frequency, magnitude) in spectrum.frequencies.iter().zip(&spectrum.magnitudes) {
        push_csv_row(
            &mut output,
            &[
                label.clone(),
                unit.to_owned(),
                csv_number(*frequency),
                csv_number(*magnitude),
            ],
        );
    }
    output
}

/// Serializes a Bode response as CSV.
pub fn bode_csv(trace: &BodeTrace) -> String {
    let mut output = String::from("signal,frequency_hz,magnitude_v,gain_db,phase_deg\n");
    let label = csv_field(&trace.signal.label());
    for point in &trace.points {
        push_csv_row(
            &mut output,
            &[
                label.clone(),
                csv_number(point.frequency),
                csv_number(point.magnitude),
                csv_number(point.gain_db),
                csv_number(point.phase_degrees),
            ],
        );
    }
    output
}

fn push_csv_row(output: &mut String, fields: &[String]) {
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str(field);
    }
    output.push('\n');
}

fn csv_field(value: &str) -> String {
    if value
        .chars()
        .any(|character| matches!(character, ',' | '"' | '\n' | '\r'))
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn csv_number(value: f64) -> String {
    format!("{value:.12e}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use editor_core::GridPos;
    use sim_core::netlist::NodeKind;

    fn demo_extraction() -> Extraction {
        Document::demo().unwrap().extraction().unwrap().clone()
    }

    fn default_signals(catalog: &SignalCatalog) -> Vec<SignalId> {
        catalog
            .voltages
            .iter()
            .cloned()
            .map(SignalId::Voltage)
            .collect()
    }

    #[test]
    fn probe_symbols_report_voltage_current_and_resistance() {
        let mut document = Document::demo().unwrap();
        let voltmeter = document
            .add_symbol(SymbolKind::Voltmeter, GridPos::new(2, -10))
            .unwrap();
        let ammeter = document
            .add_symbol(SymbolKind::Ammeter, GridPos::new(-1, -10))
            .unwrap();
        let ohmmeter = document
            .add_symbol(SymbolKind::Ohmmeter, GridPos::new(-1, -10))
            .unwrap();
        let left_net = document
            .net_name_at(GridPos::new(-3, -10))
            .expect("R1 left net");
        let right_net = document
            .net_name_at(GridPos::new(1, -10))
            .expect("R1 right net");
        let live = LiveValues {
            document_revision: document.revision(),
            time: Some(1e-3),
            node_voltages: [
                (left_net, 10.0),
                (right_net.clone(), 9.0),
                ("0".to_owned(), 0.0),
            ]
            .into_iter()
            .collect(),
            branch_currents: BTreeMap::new(),
        };
        let readings = probe_readings(&document, &live);
        let voltage = readings.iter().find(|probe| probe.id == voltmeter).unwrap();
        assert_eq!(voltage.net.as_deref(), Some(right_net.as_str()));
        assert_eq!(voltage.value, Some(9.0));
        let current = readings.iter().find(|probe| probe.id == ammeter).unwrap();
        assert_eq!(current.target.as_deref(), Some("R1"));
        assert!((current.value.unwrap() - 1e-3).abs() < 1e-12);
        let resistance = readings.iter().find(|probe| probe.id == ohmmeter).unwrap();
        assert_eq!(resistance.value, Some(1_000.0));
        assert!(readings.iter().all(|probe| probe.kind.is_measurement()));
        assert!(document.extraction().is_some());
    }

    #[test]
    fn catalog_exposes_demo_nets_and_sources() {
        let catalog = SignalCatalog::from_extraction(&demo_extraction());
        assert_eq!(catalog.voltages.len(), 3);
        assert!(catalog.voltages.contains(&"Vc".to_owned()));
        assert_eq!(catalog.currents, vec!["V1".to_owned()]);
        assert_eq!(catalog.sources.len(), 1);
        assert_eq!(catalog.sources[0].label, "V1");
        assert_eq!(catalog.sources[0].dc, 10.0);
        assert_eq!(catalog.default_voltage().as_deref(), Some("Vc"));
    }

    #[test]
    fn live_values_derive_resistor_current_and_power() {
        let document = Document::demo().unwrap();
        let resistor = document
            .schematic()
            .symbols()
            .find(|symbol| matches!(symbol.kind, SymbolKind::Resistor))
            .unwrap();
        let left_net = document
            .net_name_at(GridPos::new(-3, -10))
            .expect("R1 left net");
        let right_net = document
            .net_name_at(GridPos::new(1, -10))
            .expect("R1 right net");
        let live = LiveValues {
            document_revision: document.revision(),
            time: Some(1e-3),
            node_voltages: [
                (left_net.clone(), 10.0),
                (right_net, 9.0),
                ("0".to_owned(), 0.0),
            ]
            .into_iter()
            .collect(),
            branch_currents: BTreeMap::new(),
        };
        let observed = live.symbol(&document, resistor);
        assert_eq!(observed.pins.len(), 2);
        assert!(observed
            .pins
            .iter()
            .any(|pin| pin.net.as_deref() == Some(left_net.as_str())));
        let current = observed.current.unwrap();
        assert!((current - 1e-3).abs() < 1e-12);
        let power = observed.power.unwrap();
        assert!((power - 1e-3).abs() < 1e-12);
    }

    #[test]
    fn mosfet_live_current_matches_the_square_law_and_swap() {
        let n_params: sim_core::netlist::ParamMap = [
            ("channel".to_owned(), ParamValue::Text("n".to_owned())),
            ("vth".to_owned(), ParamValue::Number(1.6)),
            ("k".to_owned(), ParamValue::Number(0.05)),
            ("lambda".to_owned(), ParamValue::Number(0.02)),
            ("is".to_owned(), ParamValue::Number(1e-14)),
            ("n".to_owned(), ParamValue::Number(1.0)),
            ("swap_ds".to_owned(), ParamValue::Bool(false)),
        ]
        .into();
        let saturation = 0.05 * 1.4_f64.powi(2) * 1.06;
        let triode = 0.05 * (2.0 * 1.4 * 0.5 - 0.25) * 1.01;
        let measured = mosfet_current(&n_params, 3.0, 3.0).unwrap();
        assert!((measured - saturation).abs() < 1e-12);
        let measured = mosfet_current(&n_params, 0.5, 3.0).unwrap();
        assert!((measured - triode).abs() < 1e-12);
        assert!(mosfet_current(&n_params, 3.0, 1.0).unwrap().abs() < 1e-12);

        let p_params: sim_core::netlist::ParamMap = [
            ("channel".to_owned(), ParamValue::Text("p".to_owned())),
            ("vth".to_owned(), ParamValue::Number(1.6)),
            ("k".to_owned(), ParamValue::Number(0.05)),
            ("lambda".to_owned(), ParamValue::Number(0.02)),
            ("is".to_owned(), ParamValue::Number(1e-14)),
            ("n".to_owned(), ParamValue::Number(1.0)),
            ("swap_ds".to_owned(), ParamValue::Bool(false)),
        ]
        .into();
        let mirrored = mosfet_current(&p_params, -3.0, -3.0).unwrap();
        assert!((mirrored + saturation).abs() < 1e-12);

        let mut swapped = n_params.clone();
        swapped.insert("swap_ds".to_owned(), ParamValue::Bool(true));
        // D and S permuted: the symbol drain is the internal source, so the
        // same terminal bias conducts in reverse (D = 0 V, S = 0.5 V, G = 3 V).
        let reversed = mosfet_current(&swapped, -0.5, 2.5).unwrap();
        assert!((reversed + triode).abs() < 1e-9);

        let injections = symbol_terminal_injections(
            &Symbol::from_def(
                SymbolId(9),
                editor_core::SymbolRegistry::builtin()
                    .get(&SymbolKind::Mosfet)
                    .unwrap(),
                GridPos::new(0, 0),
                "M1",
                editor_core::Orientation::R0,
                false,
            ),
            0.25,
        );
        assert_eq!(injections, [-0.25, 0.0, 0.25, 0.0, 0.0]);
    }

    #[test]
    fn comparator_live_current_uses_offset_sink_and_release_states() {
        let params = [
            ("v_offset".to_owned(), ParamValue::Number(2e-3)),
            ("r_sink".to_owned(), ParamValue::Number(20.0)),
            ("imax".to_owned(), ParamValue::Number(0.02)),
        ]
        .into();

        assert_eq!(comparator_sink_current(&params, 1e-3, 0.1), Some(5e-3));
        assert_eq!(comparator_sink_current(&params, 3e-3, 0.1), Some(0.0));
        assert_eq!(comparator_sink_current(&params, 1e-3, 1.0), Some(0.02));
    }

    #[test]
    fn open_switch_current_is_reported_as_zero() {
        let document = Document::switch_example().unwrap();
        let switch = document
            .schematic()
            .symbols()
            .find(|symbol| symbol.kind == SymbolKind::Switch)
            .unwrap();
        let definition = document.registry().get(&switch.kind).unwrap();
        let a = switch
            .pin_position_of(&definition.pins[0])
            .and_then(|at| document.net_name_at(at))
            .unwrap();
        let b = switch
            .pin_position_of(&definition.pins[1])
            .and_then(|at| document.net_name_at(at))
            .unwrap();
        let live = LiveValues {
            document_revision: document.revision(),
            time: Some(1e-3),
            node_voltages: [(a, 5.0), (b, 0.0)].into_iter().collect(),
            branch_currents: BTreeMap::new(),
        };
        let observed = live.symbol(&document, switch);
        assert_eq!(observed.current, Some(0.0));
    }

    #[test]
    fn transient_settings_follow_the_source_catalog() {
        let extraction = demo_extraction();
        let catalog = SignalCatalog::from_extraction(&extraction);
        let mut settings = TransientSettings::default();
        settings.ensure_catalog(&catalog.sources);
        assert_eq!(settings.source.as_deref(), Some("V1"));
        assert_eq!(settings.final_value, "10 V");
        settings.final_value = "3".to_owned();
        settings.ensure_catalog(&catalog.sources);
        assert_eq!(settings.final_value, "3");
        settings.ensure_catalog(&[]);
        assert!(!settings.step_enabled);
        assert!(settings.source.is_none());
    }

    #[test]
    fn realtime_default_does_not_apply_a_source_step() {
        let extraction = demo_extraction();
        let settings = TransientSettings::default();
        let setup = prepare_realtime(&extraction, &settings, 0).unwrap();
        assert!(setup.options.events.is_empty());
        assert_eq!(setup.options.stop_time, f64::MAX);
    }

    #[test]
    fn transient_source_step_overrides_the_dc_value_without_mutating_the_document() {
        let extraction = demo_extraction();
        let original = extraction
            .netlist
            .components
            .iter()
            .find(|component| component.kind == ComponentKind::VoltageSource)
            .and_then(|component| component.params.get("dc"))
            .and_then(ParamValue::as_number);
        assert!(original.is_some());
        let mut settings = TransientSettings {
            step_enabled: true,
            ..TransientSettings::default()
        };
        settings.ensure_catalog(&SignalCatalog::from_extraction(&extraction).sources);
        let prepared = prepare_transient(&extraction, &settings, 4).unwrap();
        assert_eq!(prepared.context.document_revision, 4);
        assert!((prepared.context.nominal_step.unwrap() - 1e-5).abs() < 1e-15);
        let Analysis::Transient(options) = prepared.analysis else {
            panic!("expected transient analysis");
        };
        assert_eq!(options.events.len(), 1);
        assert_eq!(options.events[0].time, 0.0);
        let source = prepared
            .netlist
            .components
            .iter()
            .find(|component| component.kind == ComponentKind::VoltageSource)
            .unwrap();
        assert_eq!(
            source.params.get("dc").and_then(ParamValue::as_number),
            Some(0.0)
        );
        let unchanged = extraction
            .netlist
            .components
            .iter()
            .find(|component| component.kind == ComponentKind::VoltageSource)
            .and_then(|component| component.params.get("dc"))
            .and_then(ParamValue::as_number);
        assert_eq!(unchanged, original);
    }

    #[test]
    fn oversized_transient_is_rejected() {
        let extraction = demo_extraction();
        let settings = TransientSettings {
            step: "1n".to_owned(),
            ..TransientSettings::default()
        };
        let error = prepare_transient(&extraction, &settings, 0).unwrap_err();
        assert!(matches!(
            error,
            SimulationError::TooManySteps {
                limit: MAX_TRANSIENT_STEPS
            }
        ));
    }

    #[test]
    fn invalid_analysis_settings_are_rejected_before_submission() {
        let extraction = demo_extraction();
        let catalog = SignalCatalog::from_extraction(&extraction);
        let mut transient = TransientSettings {
            step_enabled: true,
            ..TransientSettings::default()
        };
        transient.ensure_catalog(&catalog.sources);
        transient.event_time = "5m".to_owned();
        assert!(matches!(
            prepare_transient(&extraction, &transient, 0),
            Err(SimulationError::InvalidField {
                field: "instant de l'échelon",
                ..
            })
        ));
        transient.event_time = "0".to_owned();
        transient.stop_time = "0".to_owned();
        assert!(matches!(
            prepare_transient(&extraction, &transient, 0),
            Err(SimulationError::EmptyDuration)
        ));

        let mut ac = AcSettings::default();
        ac.ensure_catalog(&catalog);
        ac.points = 1;
        assert!(matches!(
            prepare_ac(&extraction, &ac, 0),
            Err(SimulationError::InvalidPointCount { max: MAX_AC_POINTS })
        ));
        ac.points = 101;
        ac.amplitude = "0".to_owned();
        assert!(matches!(
            prepare_ac(&extraction, &ac, 0),
            Err(SimulationError::InvalidField {
                field: "amplitude d'excitation",
                ..
            })
        ));
        ac.amplitude = "1".to_owned();
        ac.start = "10".to_owned();
        ac.stop = "1".to_owned();
        assert!(matches!(
            prepare_ac(&extraction, &ac, 0),
            Err(SimulationError::InvalidField {
                field: "fréquence de fin",
                ..
            })
        ));
    }

    #[test]
    fn scope_net_current_traces_are_derived_from_samples() {
        let document = Document::demo().unwrap();
        let extraction = document.extraction().unwrap().clone();
        let catalog = SignalCatalog::from_extraction(&extraction);
        let mut settings = TransientSettings::default();
        settings.ensure_catalog(&catalog.sources);
        let prepared = prepare_transient(&extraction, &settings, 0).unwrap();
        let mut client = SimulationClient::new().unwrap();
        client.submit(prepared).unwrap();
        let run = client.poll(1.0).unwrap().unwrap();
        let AnalysisResult::Transient(result) = run.result else {
            panic!("expected transient result");
        };
        let traces = transient_traces_from_samples_with_document(
            &result.samples,
            &run.context,
            &document,
            &[SignalId::NetCurrent("Vc".to_owned())],
        )
        .unwrap();
        assert_eq!(traces.len(), 1);
        assert!(traces[0].points.len() > 400);
        assert!(traces[0].points.iter().all(|point| point[1].is_finite()));
        assert!(component_current_labels(&document).contains(&"R1".to_owned()));
        let component = transient_traces_from_samples_with_document(
            &result.samples,
            &run.context,
            &document,
            &[SignalId::Current("R1".to_owned())],
        )
        .unwrap();
        assert_eq!(component.len(), 1);
        assert!(component[0].points.len() > 400);
        assert!(component[0].points.iter().all(|point| point[1].is_finite()));
        assert!(capacitor_labels(&document).contains(&"C1".to_owned()));
        let capacitor = transient_traces_from_samples_with_document(
            &result.samples,
            &run.context,
            &document,
            &[SignalId::ComponentVoltage("C1".to_owned())],
        )
        .unwrap();
        assert_eq!(capacitor.len(), 1);
        assert!(capacitor[0].points.len() > 400);
        assert!(capacitor[0].points.iter().all(|point| point[1].is_finite()));
    }

    #[test]
    fn transient_worker_run_produces_scope_data() {
        let extraction = demo_extraction();
        let catalog = SignalCatalog::from_extraction(&extraction);
        let mut settings = TransientSettings::default();
        settings.ensure_catalog(&catalog.sources);
        let prepared = prepare_transient(&extraction, &settings, 0).unwrap();
        let mut client = SimulationClient::new().unwrap();
        let revision = client.submit(prepared).unwrap();
        let run = client.poll(1.0).unwrap().unwrap();
        assert_eq!(run.revision, revision);
        assert_eq!(run.kind, AnalysisKind::Transient);
        let AnalysisResult::Transient(result) = run.result else {
            panic!("expected transient result");
        };
        assert!(result.samples.len() > 400);
        let traces = transient_traces(&result, &run.context, &default_signals(&catalog)).unwrap();
        assert_eq!(traces.len(), 3);
        let out = traces
            .iter()
            .find(|trace| trace.signal == SignalId::Voltage("Vc".to_owned()))
            .unwrap();
        let final_voltage = out.points.last().unwrap()[1];
        assert!(final_voltage > 0.0 && final_voltage <= 10.0 + 1e-9);
        let summary = measurements(&traces);
        assert_eq!(summary.len(), 3);
    }

    #[test]
    fn ac_worker_run_produces_a_low_pass_bode() {
        let extraction = demo_extraction();
        let catalog = SignalCatalog::from_extraction(&extraction);
        let mut settings = AcSettings::default();
        settings.ensure_catalog(&catalog);
        let prepared = prepare_ac(&extraction, &settings, 0).unwrap();
        let mut client = SimulationClient::new().unwrap();
        client.submit(prepared).unwrap();
        let run = client.poll(1.0).unwrap().unwrap();
        let AnalysisResult::Ac(result) = run.result else {
            panic!("expected AC result");
        };
        let trace = bode_trace(&result, &run.context, &SignalId::Voltage("Vc".to_owned())).unwrap();
        let first = trace.points.first().unwrap();
        let last = trace.points.last().unwrap();
        assert!(first.gain_db > -3.0);
        assert!(last.gain_db < -60.0);
        assert_eq!(trace.points.len(), 101);
    }

    #[test]
    fn worker_failure_is_reported() {
        let prepared = PreparedAnalysis {
            netlist: Netlist::empty(),
            analysis: Analysis::Transient(TransientOptions::default()),
            context: RunContext {
                document_revision: 0,
                nodes: BTreeMap::new(),
                components: BTreeMap::new(),
                input_amplitude: None,
                nominal_step: Some(1e-5),
            },
        };
        let mut client = SimulationClient::new().unwrap();
        client.submit(prepared).unwrap();
        let error = client.poll(1.0).unwrap_err();
        assert!(matches!(error, SimulationError::AnalysisFailed { .. }));
        assert!(!client.has_pending());
    }

    #[test]
    fn measurements_compute_summary_values() {
        let trace = Trace {
            signal: SignalId::Voltage("OUT".to_owned()),
            points: vec![[0.0, 1.0], [1.0, 3.0]],
        };
        let summary = measurements(&[trace]);
        assert_eq!(summary.len(), 1);
        let measurement = &summary[0];
        assert_eq!(measurement.minimum, 1.0);
        assert_eq!(measurement.maximum, 3.0);
        assert_eq!(measurement.peak_to_peak, 2.0);
        assert_eq!(measurement.mean, 2.0);
        assert!((measurement.rms - 5.0_f64.sqrt()).abs() < 1e-12);
        assert_eq!(measurement.samples, 2);
    }

    #[test]
    fn spectrum_and_bode_csv_have_stable_columns() {
        let trace = Trace {
            signal: SignalId::Voltage("OUT".to_owned()),
            points: vec![[0.0, 0.0]],
        };
        let spectrum = Spectrum {
            frequencies: vec![0.0, 10.0],
            magnitudes: vec![0.0, 2.0],
            resolution: 10.0,
            sample_count: 16,
            peak: Some(SpectrumPeak {
                frequency: 10.0,
                magnitude: 2.0,
            }),
        };
        assert_eq!(
            spectrum_csv(&trace, &spectrum),
            "signal,unit,frequency_hz,magnitude\n\
             V(OUT),V,0.000000000000e0,0.000000000000e0\n\
             V(OUT),V,1.000000000000e1,2.000000000000e0\n"
        );
        let bode = BodeTrace {
            signal: SignalId::Voltage("OUT".to_owned()),
            points: vec![BodePoint {
                frequency: 1.0,
                magnitude: 1.0,
                gain_db: 0.0,
                phase_degrees: -45.0,
            }],
        };
        assert_eq!(
            bode_csv(&bode),
            "signal,frequency_hz,magnitude_v,gain_db,phase_deg\n\
             V(OUT),1.000000000000e0,1.000000000000e0,0.000000000000e0,-4.500000000000e1\n"
        );
    }

    #[test]
    fn fft_size_is_capped_for_long_transients() {
        let points = (0..20_000)
            .map(|index| [index as f64 * 1e-6, 0.0])
            .collect();
        let trace = Trace {
            signal: SignalId::Voltage("OUT".to_owned()),
            points,
        };
        let spectrum = compute_spectrum(&trace, 1e-6).unwrap();
        assert_eq!(spectrum.sample_count, MAX_FFT_POINTS);
        assert_eq!(spectrum.frequencies.len(), MAX_FFT_POINTS / 2 + 1);
        assert!(spectrum.peak.is_none());
    }

    #[test]
    fn fft_finds_a_bin_centered_sine() {
        let points = (0..=1024)
            .map(|index| {
                let time = index as f64 / 1024.0;
                [time, 2.0 * (std::f64::consts::TAU * 10.0 * time).sin()]
            })
            .collect();
        let trace = Trace {
            signal: SignalId::Voltage("OUT".to_owned()),
            points,
        };
        let spectrum = compute_spectrum(&trace, 1.0 / 1024.0).unwrap();
        assert_eq!(spectrum.sample_count, 1024);
        let peak = spectrum.peak.unwrap();
        assert!((peak.frequency - 10.0).abs() < 1e-9);
        assert!((peak.magnitude - 2.0).abs() < 0.01);
    }

    #[test]
    fn csv_output_is_deterministic_and_escaped() {
        let trace = Trace {
            signal: SignalId::Voltage("A,B".to_owned()),
            points: vec![[0.0, 1.0], [1e-3, 2.5]],
        };
        let csv = transient_csv(&[trace]);
        assert!(csv.starts_with("signal,unit,time_s,value\n"));
        assert!(csv.contains("\"V(A,B)\",V,0.000000000000e0,1.000000000000e0\n"));
        assert_eq!(&csv[..csv.len() - 1], csv.trim_end());
    }

    #[test]
    fn node_maps_include_ground() {
        let extraction = demo_extraction();
        let map = node_map(&extraction.netlist);
        let ground = extraction.netlist.ground_node_id().unwrap();
        assert_eq!(map.get("0"), Some(&ground));
        assert!(extraction
            .netlist
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::Ground && node.id == ground));
    }
}
